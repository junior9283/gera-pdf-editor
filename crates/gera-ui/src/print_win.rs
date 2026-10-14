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
    let result = Err("a impressão no Windows ainda não está disponível".to_string());
    let _ = std::fs::remove_file(&req.file);
    result
}
