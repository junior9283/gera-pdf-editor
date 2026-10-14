//! Impressão no Windows: a janela "Imprimir" do sistema e o envio das folhas pelo GDI.

/// Texto UTF-16 terminado em NUL para String.
fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: a API devolve cadeias terminadas em NUL
    unsafe {
        let len = (0..).take_while(|&i| *p.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

/// Impressoras locais e de rede e a padrão do Windows (o teste de impressão confere a lista).
#[cfg(test)]
pub fn printers() -> (Vec<String>, Option<String>) {
    use windows_sys::Win32::Graphics::Printing::{
        EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
        PRINTER_INFO_4W,
    };
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: primeira chamada só mede o tamanho; a segunda escreve no buffer desse tamanho
    let list = unsafe {
        EnumPrintersW(
            flags,
            std::ptr::null(),
            4,
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut count,
        );
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        if needed == 0
            || EnumPrintersW(
                flags,
                std::ptr::null(),
                4,
                buf.as_mut_ptr().cast(),
                needed,
                &mut needed,
                &mut count,
            ) == 0
        {
            Vec::new()
        } else {
            let infos =
                std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_4W>(), count as usize);
            infos.iter().map(|i| from_wide(i.pPrinterName)).collect()
        }
    };
    let mut len = 0u32;
    // SAFETY: mede e depois lê o nome da padrão num buffer do tamanho pedido
    let default = unsafe {
        GetDefaultPrinterW(std::ptr::null_mut(), &mut len);
        let mut name = vec![0u16; len as usize];
        (len > 0 && GetDefaultPrinterW(name.as_mut_ptr(), &mut len) != 0)
            .then(|| from_wide(name.as_ptr()))
    };
    (list, default)
}

/// Cadeia UTF-16 terminada em NUL.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Maior resolução usada ao renderizar (as impressoras de 1200 ppp recebem 600 ampliado).
const MAX_DPI: f32 = 600.0;
/// Linhas por faixa renderizada (só a faixa fica na memória).
const BAND: u32 = 512;

/// Páginas escolhidas na janela "Imprimir" (1-based).
#[derive(Debug, Clone, PartialEq)]
pub enum WinPages {
    All,
    Current,
    Ranges(Vec<(u32, u32)>),
}

/// O que a janela "Imprimir" do Windows devolveu: a impressora, as configurações do driver
/// (DEVMODE: orientação, frente e verso, cor, cópias quando o driver as faz), as páginas e as
/// cópias que o app repete quando o driver não faz.
#[derive(Debug, Clone)]
pub struct WinChoice {
    pub printer: String,
    pub devmode: Vec<u8>,
    pub pages: WinPages,
    pub copies: u16,
}

/// DEVMODE atual de `printer` (o padrão do driver), com a orientação pedida.
fn default_devmode(printer: &[u16], landscape: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Graphics::Printing::{DocumentPropertiesW, PRINTER_HANDLE};
    let (hwnd, handle) = (std::ptr::null_mut(), PRINTER_HANDLE::default());
    // SAFETY: o tamanho vem do driver e o buffer (alinhado) tem esse tamanho
    unsafe {
        let size = DocumentPropertiesW(
            hwnd,
            handle,
            printer.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
        );
        if size <= 0 {
            return None;
        }
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let dm = buf.as_mut_ptr().cast::<DEVMODEW>();
        if DocumentPropertiesW(
            hwnd,
            handle,
            printer.as_ptr(),
            dm,
            std::ptr::null(),
            DM_OUT_BUFFER,
        ) < 0
        {
            return None;
        }
        let d = &mut *dm;
        d.dmFields |= DM_ORIENTATION;
        d.Anonymous1.Anonymous1.dmOrientation = if landscape {
            DMORIENT_LANDSCAPE as i16
        } else {
            DMORIENT_PORTRAIT as i16
        };
        Some(std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), size as usize).to_vec())
    }
}

/// Mostra a janela "Imprimir" do Windows (dona: `owner`). `landscape` vem da primeira página;
/// `preset` pré-seleciona páginas (Imprimir páginas das miniaturas). `None`: cancelada.
pub fn ask(
    owner: isize,
    total: usize,
    landscape: bool,
    preset: Option<Vec<(u32, u32)>>,
) -> Result<Option<WinChoice>, String> {
    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows_sys::Win32::Graphics::Printing::GetDefaultPrinterW;
    use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    const MAX_RANGES: usize = 64;
    let total = total.max(1) as u32;
    let mut ranges = vec![PRINTPAGERANGE::default(); MAX_RANGES];
    let mut n = 0;
    for (k, (a, b)) in preset.iter().flatten().take(MAX_RANGES).enumerate() {
        ranges[k] = PRINTPAGERANGE {
            nFromPage: (*a).clamp(1, total),
            nToPage: (*b).clamp(1, total),
        };
        n = k + 1;
    }
    // SAFETY: as memórias globais e o DEVMODE são criados, lidos e liberados aqui; a estrutura
    // da janela e as faixas vivem até o fim da chamada
    unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        // a impressora padrão já com a orientação da primeira página
        let mut len = 0u32;
        GetDefaultPrinterW(std::ptr::null_mut(), &mut len);
        let mut name = vec![0u16; len as usize];
        let mut hdevmode: HGLOBAL = std::ptr::null_mut();
        if len > 0 && GetDefaultPrinterW(name.as_mut_ptr(), &mut len) != 0 {
            if let Some(dm) = default_devmode(&name, landscape) {
                hdevmode = GlobalAlloc(GMEM_MOVEABLE, dm.len());
                if !hdevmode.is_null() {
                    let p = GlobalLock(hdevmode).cast::<u8>();
                    std::ptr::copy_nonoverlapping(dm.as_ptr(), p, dm.len());
                    GlobalUnlock(hdevmode);
                }
            }
        }
        let mut pd: PRINTDLGEXW = std::mem::zeroed();
        pd.lStructSize = std::mem::size_of::<PRINTDLGEXW>() as u32;
        pd.hwndOwner = owner as _;
        pd.hDevMode = hdevmode;
        pd.Flags = PD_USEDEVMODECOPIESANDCOLLATE
            | PD_NOSELECTION
            | if n > 0 { PD_PAGENUMS } else { PD_ALLPAGES };
        pd.nPageRanges = n as u32;
        pd.nMaxPageRanges = MAX_RANGES as u32;
        pd.lpPageRanges = ranges.as_mut_ptr();
        pd.nMinPage = 1;
        pd.nMaxPage = total;
        pd.nCopies = 1;
        pd.nStartPage = START_PAGE_GENERAL;
        let hr = PrintDlgExW(&mut pd);
        let result = if hr < 0 {
            Err(format!(
                "a janela de impressão do Windows falhou (0x{:08X})",
                hr as u32
            ))
        } else if pd.dwResultAction != PD_RESULT_PRINT {
            Ok(None)
        } else {
            let names = GlobalLock(pd.hDevNames).cast::<DEVNAMES>();
            let printer = if names.is_null() {
                String::new()
            } else {
                let offset = usize::from((*names).wDeviceOffset);
                let s = from_wide(names.cast::<u16>().add(offset));
                GlobalUnlock(pd.hDevNames);
                s
            };
            let dm = GlobalLock(pd.hDevMode).cast::<windows_sys::Win32::Graphics::Gdi::DEVMODEW>();
            let devmode = if dm.is_null() {
                Vec::new()
            } else {
                let size = usize::from((*dm).dmSize) + usize::from((*dm).dmDriverExtra);
                let bytes = std::slice::from_raw_parts(dm.cast::<u8>(), size).to_vec();
                GlobalUnlock(pd.hDevMode);
                bytes
            };
            let pages = if pd.Flags & PD_CURRENTPAGE != 0 {
                WinPages::Current
            } else if pd.Flags & PD_PAGENUMS != 0 {
                WinPages::Ranges(
                    ranges[..pd.nPageRanges as usize]
                        .iter()
                        .map(|r| (r.nFromPage, r.nToPage))
                        .collect(),
                )
            } else {
                WinPages::All
            };
            Ok(Some(WinChoice {
                printer,
                devmode,
                pages,
                copies: pd.nCopies.clamp(1, 999) as u16,
            }))
        };
        for h in [pd.hDevMode, pd.hDevNames] {
            if !h.is_null() {
                GlobalFree(h);
            }
        }
        result
    }
}

/// Imprime o PDF `file` com o que a janela escolheu (numa thread); apaga o temporário em
/// qualquer caso. `current` é a página atual (1-based).
pub fn print(choice: &WinChoice, file: &std::path::Path, current: usize) -> Result<String, String> {
    // testes de interface: a impressora grava neste arquivo
    let output = cfg!(feature = "e2e")
        .then(|| std::env::var("GERA_PRINT_OUTPUT").ok())
        .flatten();
    let result = print_file(choice, file, current, output.as_deref());
    let _ = std::fs::remove_file(file);
    result
}

/// As páginas (0-based) a imprimir.
fn page_list(pages: &WinPages, current: usize, count: usize) -> Vec<usize> {
    match pages {
        WinPages::All => (0..count).collect(),
        WinPages::Current => vec![current.clamp(1, count.max(1)) - 1],
        WinPages::Ranges(r) => r
            .iter()
            .flat_map(|&(a, b)| a.min(b)..=a.max(b))
            .filter(|&p| p >= 1 && (p as usize) <= count)
            .map(|p| p as usize - 1)
            .collect(),
    }
}

fn print_file(
    choice: &WinChoice,
    file: &std::path::Path,
    current: usize,
    output: Option<&str>,
) -> Result<String, String> {
    use app_state::print_layout::sheets;
    let doc = pdf_core::PdfDoc::open(file, None).map_err(|e| e.to_string())?;
    let count = doc.page_count().map_err(|e| e.to_string())?.max(0) as usize;
    let sizes: Vec<(f32, f32)> = (0..count as i32)
        .map(|p| {
            doc.page_size(p)
                .map(|s| (s.width, s.height))
                .unwrap_or((612.0, 792.0))
        })
        .collect();
    let pages = page_list(&choice.pages, current, count);
    if pages.is_empty() {
        return Err("Nenhuma página a imprimir.".into());
    }
    let mut dc = PrinterDc::new(&wide(&choice.printer), &choice.devmode)?;
    // cada página ajustada à área imprimível, uma por folha (páginas por folha: do driver)
    let options = app_state::print::PrintOptions::default();
    let area = dc.printable();
    let sheets = sheets(&options, &pages, &sizes, area, area.w > area.h);
    let name = file.file_name().map_or_else(
        || "Gera PDF Editor".into(),
        |n| n.to_string_lossy().into_owned(),
    );
    dc.start(&name, output)?;
    for _ in 0..choice.copies.max(1) {
        for sheet in &sheets {
            dc.page(|dc| {
                for p in sheet {
                    dc.draw(&doc, p.page, sizes[p.page], p.at)?;
                }
                Ok(())
            })?;
        }
    }
    dc.end()?;
    Ok(String::new())
}

/// O contexto de dispositivo da impressora, com o documento aberto ou não.
struct PrinterDc {
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    started: bool,
    dpi: (f32, f32),
    offset: (f32, f32),
    size: (f32, f32),
}

impl PrinterDc {
    /// O DC de `printer` com as configurações `devmode` da janela "Imprimir" (vazio ou curto
    /// demais: as do driver).
    fn new(printer: &[u16], devmode: &[u8]) -> Result<Self, String> {
        use windows_sys::Win32::Graphics::Gdi::*;
        // o DEVMODE vai num buffer alinhado
        let mut buf = vec![0u64; devmode.len().div_ceil(8).max(1)];
        let header = std::mem::offset_of!(DEVMODEW, dmFields) + 4;
        let dm = if devmode.len() >= header {
            // SAFETY: cópia de bytes para um buffer alinhado de tamanho suficiente
            unsafe {
                std::ptr::copy_nonoverlapping(
                    devmode.as_ptr(),
                    buf.as_mut_ptr().cast::<u8>(),
                    devmode.len(),
                );
            }
            buf.as_ptr().cast::<DEVMODEW>()
        } else {
            std::ptr::null()
        };
        // SAFETY: nomes e DEVMODE válidos durante a chamada
        let hdc = unsafe {
            CreateDCW(
                wide("WINSPOOL").as_ptr(),
                printer.as_ptr(),
                std::ptr::null(),
                dm,
            )
        };
        if hdc.is_null() {
            return Err("não foi possível abrir a impressora".into());
        }
        // SAFETY: DC válido
        let cap = |i: u32| unsafe { GetDeviceCaps(hdc, i as i32) } as f32;
        Ok(Self {
            hdc,
            started: false,
            dpi: (cap(LOGPIXELSX).max(72.0), cap(LOGPIXELSY).max(72.0)),
            offset: (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY)),
            size: (cap(HORZRES), cap(VERTRES)),
        })
    }

    /// A parte imprimível do papel, em pontos.
    fn printable(&self) -> app_state::print_layout::Area {
        let pt = |px: f32, dpi: f32| px * 72.0 / dpi;
        app_state::print_layout::Area {
            x: pt(self.offset.0, self.dpi.0),
            y: pt(self.offset.1, self.dpi.1),
            w: pt(self.size.0, self.dpi.0),
            h: pt(self.size.1, self.dpi.1),
        }
    }

    /// Abre o documento na impressora; `output` grava num arquivo (impressoras de arquivo).
    fn start(&mut self, name: &str, output: Option<&str>) -> Result<(), String> {
        use windows_sys::Win32::Storage::Xps::{StartDocW, DOCINFOW};
        let name = wide(name);
        let output = output.map(wide);
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: name.as_ptr(),
            lpszOutput: output.as_ref().map_or(std::ptr::null(), |o| o.as_ptr()),
            lpszDatatype: std::ptr::null(),
            fwType: 0,
        };
        // SAFETY: DC válido e DOCINFO com cadeias vivas até o fim da chamada
        if unsafe { StartDocW(self.hdc, &info) } <= 0 {
            return Err("a impressora recusou o documento".into());
        }
        self.started = true;
        Ok(())
    }

    /// Uma folha: `draw` desenha as páginas entre StartPage e EndPage.
    fn page(&mut self, draw: impl FnOnce(&mut Self) -> Result<(), String>) -> Result<(), String> {
        use windows_sys::Win32::Storage::Xps::{EndPage, StartPage};
        // SAFETY: documento aberto no DC
        if unsafe { StartPage(self.hdc) } <= 0 {
            return Err("a impressora recusou a folha".into());
        }
        draw(self)?;
        // SAFETY: a folha aberta acima
        if unsafe { EndPage(self.hdc) } <= 0 {
            return Err("a impressora recusou a folha".into());
        }
        Ok(())
    }

    /// Desenha a página em `at` (pontos no papel), renderizada em faixas.
    fn draw(
        &mut self,
        doc: &pdf_core::PdfDoc,
        page: usize,
        size: (f32, f32),
        at: app_state::print_layout::Area,
    ) -> Result<(), String> {
        use windows_sys::Win32::Graphics::Gdi::*;
        // destino em pixels do dispositivo (a origem do DC é o canto da parte imprimível)
        let dx = (at.x * self.dpi.0 / 72.0 - self.offset.0).round() as i32;
        let dy = (at.y * self.dpi.1 / 72.0 - self.offset.1).round() as i32;
        let dw = (at.w * self.dpi.0 / 72.0).round().max(1.0);
        let dh = (at.h * self.dpi.1 / 72.0).round().max(1.0);
        // escala do MuPDF (px por ponto) na resolução da impressora, no máximo MAX_DPI
        let scale = (dw / size.0.max(1.0)).min(at.w * MAX_DPI / 72.0 / size.0.max(1.0));
        let page = page as i32;
        let mut y0 = 0u32;
        // SAFETY: DC válido; o modo de ampliação só afeta este DC
        unsafe {
            SetStretchBltMode(self.hdc, HALFTONE);
        }
        loop {
            let band = match doc.render_band(page, scale, y0, BAND) {
                Ok(b) => b,
                Err(_) if y0 > 0 => break, // passou do fim da página
                Err(e) => return Err(e.to_string()),
            };
            let total_h = size.1 * scale; // altura renderizada da página inteira
            let (bw, bh) = (band.width as usize, band.height as usize);
            let stride = (bw * 3).div_ceil(4) * 4;
            let mut bits = vec![0u8; stride * bh];
            for (row, src) in band.rgb.chunks_exact(bw * 3).enumerate() {
                let dst = &mut bits[row * stride..row * stride + bw * 3];
                for (d, s) in dst
                    .as_chunks_mut::<3>()
                    .0
                    .iter_mut()
                    .zip(src.as_chunks::<3>().0)
                {
                    *d = [s[2], s[1], s[0]]; // o DIB é BGR
                }
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bw as i32,
                    biHeight: -(bh as i32), // de cima para baixo
                    biPlanes: 1,
                    biBitCount: 24,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let top = dy + (y0 as f32 * dh / total_h).round() as i32;
            let bottom = dy + ((y0 as f32 + bh as f32) * dh / total_h).round() as i32;
            // SAFETY: bits com `stride` × `bh` bytes, descritos pelo BITMAPINFO acima
            let ok = unsafe {
                StretchDIBits(
                    self.hdc,
                    dx,
                    top,
                    dw as i32,
                    (bottom - top).max(1),
                    0,
                    0,
                    bw as i32,
                    bh as i32,
                    bits.as_ptr().cast(),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                )
            };
            if ok == 0 {
                return Err("a impressora recusou a imagem da página".into());
            }
            y0 += band.height;
            if band.height < BAND {
                break;
            }
        }
        Ok(())
    }

    fn end(mut self) -> Result<(), String> {
        use windows_sys::Win32::Storage::Xps::EndDoc;
        self.started = false;
        // SAFETY: documento aberto no DC
        if unsafe { EndDoc(self.hdc) } <= 0 {
            return Err("a impressora não terminou o documento".into());
        }
        Ok(())
    }
}

impl Drop for PrinterDc {
    fn drop(&mut self) {
        use windows_sys::Win32::Graphics::Gdi::DeleteDC;
        use windows_sys::Win32::Storage::Xps::AbortDoc;
        // SAFETY: o DC é deste objeto; um documento pela metade (erro) é cancelado
        unsafe {
            if self.started {
                AbortDoc(self.hdc);
            }
            DeleteDC(self.hdc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_from_the_dialog() {
        assert_eq!(page_list(&WinPages::All, 1, 3), [0, 1, 2]);
        assert_eq!(page_list(&WinPages::Current, 2, 3), [1]);
        assert_eq!(
            page_list(&WinPages::Ranges(vec![(3, 2), (5, 9)]), 1, 5),
            [1, 2, 4]
        );
    }

    /// No Windows de teste (por SSH): imprime numa impressora que grava em arquivo e confere.
    /// `GERA_TEST_PRINTER` escolhe a impressora.
    #[test]
    #[ignore = "precisa de uma impressora que grave em arquivo (rodar no Windows)"]
    fn prints_the_chosen_pages_landscape() {
        let (list, _) = printers();
        let printer = std::env::var("GERA_TEST_PRINTER").unwrap_or("Microsoft Print to PDF".into());
        assert!(list.contains(&printer), "{list:?}");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Ação 5 páginas.pdf");
        std::fs::write(&file, pdf_core::sample::sample_pdf(5, 0)).unwrap();
        let out = dir.path().join("saída.pdf");
        let choice = WinChoice {
            devmode: default_devmode(&wide(&printer), true).unwrap(),
            printer,
            pages: WinPages::Ranges(vec![(1, 3)]),
            copies: 1,
        };
        print_file(&choice, &file, 1, out.to_str()).unwrap();
        // o spooler grava o arquivo depois: espera até 60 s
        let t = std::time::Instant::now();
        while pdf_core::PdfDoc::open(&out, None).is_err() && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let printed = pdf_core::PdfDoc::open(&out, None).unwrap();
        assert_eq!(printed.page_count().unwrap(), 3);
        let size = printed.page_size(0).unwrap();
        assert!(size.width > size.height, "DEVMODE em paisagem");
    }
}
