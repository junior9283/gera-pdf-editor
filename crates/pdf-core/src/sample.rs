//! Gera PDFs mínimos para testes, exemplos e medições.

/// `rotate` é o valor de /Rotate (0, 90, 180, 270). Cada página tem o texto
/// "Page N Hello Gera" e há um índice com um item "Capitulo N" por página.
pub fn sample_pdf(pages: usize, rotate: i32) -> Vec<u8> {
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
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {rotate} /Contents {content} 0 R \
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
    let mut out = b"%PDF-1.4\n".to_vec();
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
