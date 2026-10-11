//! Gera PDFs mínimos para testes, exemplos e medições.

/// `rotate` é o valor de /Rotate (0, 90, 180, 270). Cada página tem o texto
/// "Page N Hello Gera" e há um índice com um item "Capitulo N" por página.
pub fn sample_pdf(pages: usize, rotate: i32) -> Vec<u8> {
    sample_pdf_sized(pages, rotate, (612, 792))
}

/// Como `sample_pdf`, com `size` = (largura, altura) da MediaBox em pontos.
pub fn sample_pdf_sized(pages: usize, rotate: i32, size: (u32, u32)) -> Vec<u8> {
    let (w, h) = size;
    // Numeração dos objetos: 1 catálogo, 2 páginas, 3 fonte; cada página ocupa dois (página + conteúdo).
    let page_obj = |i: usize| 4 + 2 * i;
    let outlines_obj = page_obj(pages);
    let mut objs: Vec<String> = Vec::new();
    let outlines_ref = if pages > 0 {
        format!(" /Outlines {outlines_obj} 0 R")
    } else {
        String::new()
    };
    objs.push(format!("<< /Type /Catalog /Pages 2 0 R{outlines_ref} >>"));
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", page_obj(i))).collect();
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..pages {
        let content = page_obj(i) + 1;
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Rotate {rotate} /Contents {content} 0 R \
             /Resources << /Font << /F1 3 0 R >> >> >>"
        ));
        let stream = format!("BT /F1 24 Tf 72 700 Td (Page {} Hello Gera) Tj ET", i + 1);
        objs.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    if pages > 0 {
        let first = outlines_obj + 1;
        let last = outlines_obj + pages;
        objs.push(format!(
            "<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {pages} >>"
        ));
        for i in 0..pages {
            let me = first + i;
            let prev = if i > 0 {
                format!(" /Prev {} 0 R", me - 1)
            } else {
                String::new()
            };
            let next = if i + 1 < pages {
                format!(" /Next {} 0 R", me + 1)
            } else {
                String::new()
            };
            objs.push(format!(
                "<< /Title (Capitulo {}) /Parent {outlines_obj} 0 R /Dest [{} 0 R /Fit]{prev}{next} >>",
                i + 1,
                page_obj(i)
            ));
        }
    }
    pdf_from_objects(&objs)
}

/// PDF com os objetos dados (o objeto `i` é `objs[i - 1]`; o 1 é o catálogo), com xref e trailer.
pub fn pdf_from_objects(objs: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in &offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Pixel mais escuro da imagem (soma dos 3 canais): referência para conferir a rotação.
pub fn darkest_pixel(img: &crate::RenderedPage) -> (usize, usize) {
    let w = img.width as usize;
    let i = img
        .rgb
        .chunks(3)
        .enumerate()
        .min_by_key(|(_, p)| p.iter().map(|&b| u32::from(b)).sum::<u32>())
        .map_or(0, |(i, _)| i);
    (i % w.max(1), i / w.max(1))
}

/// Há tinta escura em volta de (x, y)? (vizinhança 3x3, por causa da suavização das bordas)
pub fn dark_near(img: &crate::RenderedPage, x: usize, y: usize) -> bool {
    let (w, h) = (img.width as usize, img.height as usize);
    (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| {
        (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| {
            let i = (yy * w + xx) * 3;
            img.rgb[i..i + 3].iter().map(|&b| u32::from(b)).sum::<u32>() < 300
        })
    })
}

/// PDF de 1 página com um formulário AcroForm: texto "nome", caixa "aceito", botões de opção
/// "plano" (básico/completo), lista suspensa "estado" e lista "cores" (com /NeedAppearances).
pub fn form_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 9 0 R 10 0 R] /NeedAppearances true /DA (/Helv 12 Tf 0 g) /DR << /Font << /Helv 11 0 R /ZaDb 12 0 R >> >> >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R 5 0 R 7 0 R 8 0 R 9 0 R 10 0 R] /Resources << /Font << /Helv 11 0 R >> >> >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (nome) /V (Ana) /Rect [100 700 300 720] /DA (/Helv 12 Tf 0 g) /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (aceito) /V /Off /AS /Off /Rect [100 650 115 665] /MK << /CA (4) >> /DA (/ZaDb 0 Tf 0 g) /AP << /N << /Sim 13 0 R /Off 14 0 R >> >> /P 3 0 R /F 4 >>".to_owned(),
        "<< /FT /Btn /Ff 49152 /T (plano) /V /Off /Kids [7 0 R 8 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /Parent 6 0 R /AS /Off /Rect [100 600 115 615] /MK << /CA (l) >> /DA (/ZaDb 0 Tf 0 g) /AP << /N << /basico 13 0 R /Off 14 0 R >> >> /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /Parent 6 0 R /AS /Off /Rect [200 600 215 615] /MK << /CA (l) >> /DA (/ZaDb 0 Tf 0 g) /AP << /N << /completo 13 0 R /Off 14 0 R >> >> /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (estado) /V (SP) /Opt [(SP) (RJ) (MG)] /Rect [100 550 200 570] /DA (/Helv 12 Tf 0 g) /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Ch /T (cores) /Opt [[(az) (Azul)] [(vm) (Vermelho)]] /Rect [100 450 200 520] /DA (/Helv 12 Tf 0 g) /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>".to_owned(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 15 15] /Resources << /Font << /ZaDb 12 0 R >> >> /Length 34 >>\nstream\nBT /ZaDb 12 Tf 2 3 Td (4) Tj ET\nendstream".to_owned(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 15 15] /Length 0 >>\nstream\n\nendstream".to_owned(),
    ];
    pdf_from_objects(&objs)
}
