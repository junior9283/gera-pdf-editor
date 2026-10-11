//! Ícone do app (janela, barra de tarefas e AppImage): `packaging/icons`.

const ICON_PNG: &[u8] = include_bytes!("../../../packaging/icons/gera-pdf-editor-128.png");

/// O ícone da janela, decodificado do PNG embutido.
pub fn app_icon() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(ICON_PNG))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf[..frame.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..frame.buffer_size()]
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some(egui::IconData {
        rgba,
        width: frame.width,
        height: frame.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_is_a_square_rgba_image() {
        let icon = app_icon().expect("o PNG embutido decodifica");
        assert_eq!((icon.width, icon.height), (128, 128));
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);
        // fundo transparente nos cantos, desenho no meio
        assert_eq!(icon.rgba[3], 0, "canto transparente");
        let center = (64 * 128 + 64) * 4;
        assert!(icon.rgba[center + 3] >= 250, "meio opaco");
    }
}
