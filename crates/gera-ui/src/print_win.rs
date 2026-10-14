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
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W,
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
    (list, default_printer().map(|n| from_wide(n.as_ptr())))
}

/// Nome da impressora padrão do Windows (UTF-16 com NUL).
fn default_printer() -> Option<Vec<u16>> {
    use windows_sys::Win32::Graphics::Printing::GetDefaultPrinterW;
    let mut len = 0u32;
    // SAFETY: mede e depois lê o nome num buffer do tamanho pedido
    unsafe {
        GetDefaultPrinterW(std::ptr::null_mut(), &mut len);
        let mut name = vec![0u16; len as usize];
        (len > 0 && GetDefaultPrinterW(name.as_mut_ptr(), &mut len) != 0).then_some(name)
    }
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
    /// Agrupar: as cópias saem como documentos inteiros (1, 2, 3, 1, 2, 3).
    pub collate: bool,
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
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
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
        let mut hdevmode: HGLOBAL = std::ptr::null_mut();
        if let Some(name) = default_printer() {
            if let Some(dm) = default_devmode(&name, landscape) {
                hdevmode = GlobalAlloc(GMEM_MOVEABLE, dm.len());
                let p = if hdevmode.is_null() {
                    std::ptr::null_mut()
                } else {
                    GlobalLock(hdevmode).cast::<u8>()
                };
                if p.is_null() {
                    if !hdevmode.is_null() {
                        GlobalFree(hdevmode);
                    }
                    hdevmode = std::ptr::null_mut();
                } else {
                    std::ptr::copy_nonoverlapping(dm.as_ptr(), p, dm.len());
                    GlobalUnlock(hdevmode);
                }
            }
        }
        let mut pd: PRINTDLGEXW = std::mem::zeroed();
        pd.lStructSize = std::mem::size_of::<PRINTDLGEXW>() as u32;
        pd.hwndOwner = owner as _;
        pd.hDevMode = hdevmode;
        // as cópias e o agrupar ficam com o app (com a flag de usar os do DEVMODE, a janela
        // desabilita as cópias quando o driver não as faz)
        pd.Flags = PD_NOSELECTION | if n > 0 { PD_PAGENUMS } else { PD_ALLPAGES };
        pd.nPageRanges = n as u32;
        pd.nMaxPageRanges = MAX_RANGES as u32;
        pd.lpPageRanges = ranges.as_mut_ptr();
        pd.nMinPage = 1;
        pd.nMaxPage = total;
        pd.nCopies = 1;
        pd.nStartPage = START_PAGE_GENERAL;
        let hr = PrintDlgExW(&mut pd);
        CoUninitialize();
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
                collate: pd.Flags & PD_COLLATE != 0,
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
    let doc = pdf_core::PdfDoc::open(file, None).map_err(|e| e.to_string())?;
    let count = doc.page_count().map_err(|e| e.to_string())?.max(0) as usize;
    let pages = page_list(&choice.pages, current, count);
    if pages.is_empty() {
        return Err("Nenhuma página a imprimir.".into());
    }
    let mut dc = PrinterDc::new(&wide(&choice.printer), &choice.devmode)?;
    let name = file.file_name().map_or_else(
        || "Gera PDF Editor".into(),
        |n| n.to_string_lossy().into_owned(),
    );
    dc.start(&name, output)?;
    // uma página por folha, ajustada à área imprimível (páginas por folha: as do driver); as
    // cópias agrupadas repetem o documento, as outras repetem cada página
    let copies = usize::from(choice.copies.max(1));
    let order: Vec<usize> = if choice.collate {
        (0..copies).flat_map(|_| pages.iter().copied()).collect()
    } else {
        pages
            .iter()
            .flat_map(|&p| std::iter::repeat_n(p, copies))
            .collect()
    };
    for page in order {
        let size = doc.page_size(page as i32).map_err(|e| e.to_string())?;
        dc.page(|dc| dc.draw(&doc, page as i32, (size.width, size.height)))?;
    }
    dc.end()?;
    Ok(String::new())
}

/// O contexto de dispositivo da impressora, com o documento aberto ou não.
struct PrinterDc {
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    started: bool,
    dpi: (f32, f32),
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
            // as cópias são feitas pelo app: o driver imprime uma
            if devmode.len() >= std::mem::offset_of!(DEVMODEW, dmColor) {
                // SAFETY: o buffer cobre os campos públicos até dmCopies
                unsafe {
                    let d = &mut *buf.as_mut_ptr().cast::<DEVMODEW>();
                    d.dmFields |= DM_COPIES;
                    d.Anonymous1.Anonymous1.dmCopies = 1;
                }
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
            size: (cap(HORZRES), cap(VERTRES)),
        })
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

    /// Desenha a página (`size` em pontos) ajustada e centrada na área imprimível,
    /// rasterizada em faixas na resolução da impressora (no máximo `MAX_DPI`).
    fn draw(&mut self, doc: &pdf_core::PdfDoc, page: i32, size: (f32, f32)) -> Result<(), String> {
        use windows_sys::Win32::Graphics::Gdi::*;
        // retrato/paisagem automático, como o Acrobat: a página deitada na folha em pé (ou o
        // contrário) é girada para ocupar a folha
        let turn = (size.0 > size.1) != (self.size.0 > self.size.1) && size.0 != size.1;
        let (pw, ph) = if turn { (size.1, size.0) } else { size };
        let (pw, ph) = (pw.max(1.0), ph.max(1.0));
        // escala que cabe na área imprimível, em pontos do papel por ponto da página
        let fit = (self.size.0 * 72.0 / self.dpi.0 / pw).min(self.size.1 * 72.0 / self.dpi.1 / ph);
        // destino em pixels do dispositivo (a origem do DC é o canto da área imprimível)
        let dw = (pw * fit * self.dpi.0 / 72.0).round().max(1.0);
        let dh = (ph * fit * self.dpi.1 / 72.0).round().max(1.0);
        let dx = ((self.size.0 - dw) / 2.0).round() as i32;
        let dy = ((self.size.1 - dh) / 2.0).round() as i32;
        let renderer = doc
            .band_renderer(page, fit * self.dpi.0.min(MAX_DPI) / 72.0, turn)
            .map_err(|e| e.to_string())?;
        let (bw, total_h) = renderer.size();
        // SAFETY: DC válido; o modo de ampliação só afeta este DC
        unsafe {
            SetStretchBltMode(self.hdc, HALFTONE);
        }
        let mut y0 = 0u32;
        while y0 < total_h {
            let (bh, bits) = renderer.band_bgra(y0, BAND).map_err(|e| e.to_string())?;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bw as i32,
                    biHeight: -(bh as i32), // de cima para baixo
                    biPlanes: 1,
                    biBitCount: 32, // BGRA: linhas já alinhadas, sem conversão
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let row = |y: u32| dy + (y as f32 * dh / total_h as f32).round() as i32;
            let (top, bottom) = (row(y0), row(y0 + bh));
            // SAFETY: `bits` tem bw × bh × 4 bytes, descritos pelo BITMAPINFO acima
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
            y0 += bh;
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
            copies: 2,
            collate: true,
        };
        print_file(&choice, &file, 1, out.to_str()).unwrap();
        // o spooler grava o arquivo depois: espera até 60 s
        let t = std::time::Instant::now();
        while pdf_core::PdfDoc::open(&out, None).is_err() && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let printed = pdf_core::PdfDoc::open(&out, None).unwrap();
        assert_eq!(printed.page_count().unwrap(), 6, "3 páginas × 2 cópias");
        let size = printed.page_size(0).unwrap();
        assert!(size.width > size.height, "DEVMODE em paisagem");
    }

    /// Páginas deitadas numa folha em pé: o app gira cada uma (o resultado é todo em pé, com o
    /// conteúdo ocupando a folha).
    #[test]
    #[ignore = "precisa de uma impressora que grave em arquivo (rodar no Windows)"]
    fn landscape_pages_are_turned_on_portrait_paper() {
        let printer = std::env::var("GERA_TEST_PRINTER").unwrap_or("Microsoft Print to PDF".into());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deitado.pdf");
        std::fs::write(&file, pdf_core::sample::sample_pdf(2, 90)).unwrap();
        let out = dir.path().join("saída.pdf");
        let choice = WinChoice {
            devmode: default_devmode(&wide(&printer), false).unwrap(),
            printer,
            pages: WinPages::All,
            copies: 1,
            collate: false,
        };
        print_file(&choice, &file, 1, out.to_str()).unwrap();
        let t = std::time::Instant::now();
        while pdf_core::PdfDoc::open(&out, None).is_err() && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let printed = pdf_core::PdfDoc::open(&out, None).unwrap();
        assert_eq!(printed.page_count().unwrap(), 2);
        let size = printed.page_size(0).unwrap();
        assert!(size.width < size.height, "folha em pé");
        std::fs::copy(&out, "C:\\gera\\girado.pdf").unwrap();
    }
}
