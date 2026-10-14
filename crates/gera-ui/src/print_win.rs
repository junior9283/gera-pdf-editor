//! Impressão no Windows (GDI): as impressoras do sistema e o envio das folhas.
use crate::print_job::PrintRequest;
use windows_sys::Win32::Graphics::Printing::{
    EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    PRINTER_INFO_4W,
};

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

/// Impressoras locais e de rede e a padrão do Windows.
pub fn printers() -> (Vec<String>, Option<String>) {
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

/// Imprime o pedido (numa thread); apaga o temporário em qualquer caso.
pub fn print(req: &PrintRequest) -> Result<String, String> {
    // testes de interface: a impressora "Microsoft Print to PDF" grava neste arquivo
    let output = cfg!(feature = "e2e")
        .then(|| std::env::var("GERA_PRINT_OUTPUT").ok())
        .flatten();
    let result = print_file(req, output.as_deref());
    let _ = std::fs::remove_file(&req.file);
    result
}

/// Cadeia UTF-16 terminada em NUL.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Maior resolução usada ao renderizar (as impressoras de 1200 ppp recebem 600 ampliado).
const MAX_DPI: f32 = 600.0;
/// Linhas por faixa renderizada (só a faixa fica na memória).
const BAND: u32 = 512;

fn print_file(req: &PrintRequest, output: Option<&str>) -> Result<String, String> {
    use app_state::print_layout::{landscape, page_list, sheets};
    let o = &req.options;
    let doc = pdf_core::PdfDoc::open(&req.file, None).map_err(|e| e.to_string())?;
    let count = doc.page_count().map_err(|e| e.to_string())?.max(0) as usize;
    let sizes: Vec<(f32, f32)> = (0..count as i32)
        .map(|p| {
            doc.page_size(p)
                .map(|s| (s.width, s.height))
                .unwrap_or((612.0, 792.0))
        })
        .collect();
    let pages = page_list(o, req.current, req.total.min(count))?;
    let lay = landscape(o, sizes[pages[0]]);
    let printer = wide(&o.printer);
    let mut dc = PrinterDc::new(&printer, o.copies.max(1), lay, o.duplex, o.grayscale)?;
    let area = dc.printable();
    let sheets = sheets(o, &pages, &sizes, area, lay);
    // a impressora que não faz cópias sozinha recebe as folhas repetidas (agrupadas)
    let rounds = if dc.driver_copies { 1 } else { o.copies.max(1) };
    let name = req.file.file_name().map_or_else(
        || "Gera PDF Editor".into(),
        |n| n.to_string_lossy().into_owned(),
    );
    dc.start(&name, output)?;
    for _ in 0..rounds {
        for sheet in &sheets {
            dc.page(|dc| {
                for p in sheet {
                    dc.draw(&doc, p.page, sizes[p.page], p.at, o.grayscale)?;
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
    driver_copies: bool,
    dpi: (f32, f32),
    offset: (f32, f32),
    size: (f32, f32),
}

impl PrinterDc {
    fn new(
        printer: &[u16],
        copies: u16,
        landscape: bool,
        duplex: app_state::print::Duplex,
        grayscale: bool,
    ) -> Result<Self, String> {
        use app_state::print::Duplex;
        use windows_sys::Win32::Graphics::Gdi::*;
        use windows_sys::Win32::Graphics::Printing::{DocumentPropertiesW, PRINTER_HANDLE};
        use windows_sys::Win32::Storage::Xps::{DeviceCapabilitiesW, DC_COPIES};
        let (hwnd, handle) = (std::ptr::null_mut(), PRINTER_HANDLE::default());
        // SAFETY: o DEVMODE do driver é lido num buffer do tamanho que ele pede, alterado nos
        // campos públicos e validado pelo próprio driver antes de criar o DC
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
                return Err("a impressora não foi encontrada".into());
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
                return Err("a impressora não respondeu".into());
            }
            let max_copies = DeviceCapabilitiesW(
                printer.as_ptr(),
                std::ptr::null(),
                DC_COPIES,
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            let driver_copies = copies == 1 || max_copies >= i32::from(copies);
            let d = &mut *dm;
            d.dmFields |= DM_ORIENTATION | DM_DUPLEX | DM_COLOR | DM_COPIES | DM_COLLATE;
            d.Anonymous1.Anonymous1.dmOrientation = if landscape {
                DMORIENT_LANDSCAPE as i16
            } else {
                DMORIENT_PORTRAIT as i16
            };
            d.Anonymous1.Anonymous1.dmCopies = if driver_copies { copies as i16 } else { 1 };
            d.dmCollate = DMCOLLATE_TRUE;
            d.dmDuplex = match duplex {
                Duplex::None => DMDUP_SIMPLEX,
                // a borda longa é a "vertical" do Windows no papel em pé (e a horizontal deitado)
                Duplex::LongEdge if landscape => DMDUP_HORIZONTAL,
                Duplex::LongEdge => DMDUP_VERTICAL,
                Duplex::ShortEdge if landscape => DMDUP_VERTICAL,
                Duplex::ShortEdge => DMDUP_HORIZONTAL,
            };
            d.dmColor = if grayscale {
                DMCOLOR_MONOCHROME
            } else {
                DMCOLOR_COLOR
            };
            DocumentPropertiesW(
                hwnd,
                handle,
                printer.as_ptr(),
                dm,
                dm,
                DM_IN_BUFFER | DM_OUT_BUFFER,
            );
            let hdc = CreateDCW(
                wide("WINSPOOL").as_ptr(),
                printer.as_ptr(),
                std::ptr::null(),
                dm,
            );
            if hdc.is_null() {
                return Err("não foi possível abrir a impressora".into());
            }
            let cap = |i: u32| GetDeviceCaps(hdc, i as i32) as f32;
            let dpi = (cap(LOGPIXELSX).max(72.0), cap(LOGPIXELSY).max(72.0));
            Ok(Self {
                hdc,
                started: false,
                driver_copies,
                dpi,
                offset: (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY)),
                size: (cap(HORZRES), cap(VERTRES)),
            })
        }
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
        gray: bool,
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
                    if gray {
                        let l = ((u32::from(s[0]) * 299
                            + u32::from(s[1]) * 587
                            + u32::from(s[2]) * 114)
                            / 1000) as u8;
                        d.copy_from_slice(&[l, l, l]);
                    } else {
                        d.copy_from_slice(&[s[2], s[1], s[0]]); // o DIB é BGR
                    }
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

    /// No Windows de teste (por SSH): imprime numa impressora de PDF e confere o arquivo.
    /// `GERA_TEST_PRINTER` escolhe a impressora (padrão "Microsoft Print to PDF", que grava onde
    /// o app manda); `GERA_TEST_PRINT_OUT`, o arquivo que ela grava (a "Adobe PDF" grava em
    /// Documentos com o nome do documento).
    #[test]
    #[ignore = "precisa de uma impressora de PDF (rodar no Windows)"]
    fn prints_two_per_sheet_to_the_pdf_printer() {
        let (list, default) = printers();
        let printer = std::env::var("GERA_TEST_PRINTER").unwrap_or("Microsoft Print to PDF".into());
        assert!(list.contains(&printer), "{list:?}");
        assert!(default.is_some_and(|d| list.contains(&d)));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Ação 5 páginas.pdf");
        std::fs::write(&file, pdf_core::sample::sample_pdf(5, 0)).unwrap();
        let out = std::env::var("GERA_TEST_PRINT_OUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| dir.path().join("saída.pdf"));
        let _ = std::fs::remove_file(&out);
        let mut options = app_state::print::PrintOptions {
            printer,
            per_sheet: 2,
            ..Default::default()
        };
        options.pages = app_state::print::Pages::Ranges;
        options.ranges = "1-3".into();
        let req = PrintRequest {
            options,
            current: 1,
            total: 5,
            oversized: false,
            file,
        };
        print_file(&req, out.to_str()).unwrap();
        // o spooler grava o arquivo depois: espera até 60 s
        let t = std::time::Instant::now();
        while pdf_core::PdfDoc::open(&out, None).is_err() && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let printed = pdf_core::PdfDoc::open(&out, None).unwrap();
        assert_eq!(printed.page_count().unwrap(), 2, "3 páginas, 2 por folha");
        let size = printed.page_size(0).unwrap();
        assert!(size.width > size.height, "2 por folha: papel deitado");
    }
}
