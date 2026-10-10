# Gera PDF Editor — Subprojeto 4: Editar PDF (texto e imagens)

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Editar PDF** do Acrobat: no modo de edição, os blocos de texto e as imagens da página
aparecem com contorno; dá para mexer em imagens (v4a) e reescrever o texto (v4b).

## 2. v4a — Imagens

- Modo Editar PDF (linha da ferramenta): contornos das imagens da página; clique seleciona (alças),
  arrastar move, alças redimensionam (proporção mantida, como no Acrobat), Delete exclui, setas movem;
  **Girar** 90° (horário/anti-horário), **Substituir imagem…**, **Adicionar imagem…** (clique coloca).
- Técnica: a imagem é o XObject chamado no conteúdo da página (`/Nome Do`). Mover/redimensionar/
  girar = tirar aquele `Do` do conteúdo e desenhá-lo de novo (`q matriz cm /Nome Do Q`) no fim — o
  objeto da imagem fica o mesmo (máscaras, perfis e compressão preservados) e nada mais é tocado.
  Substituir = imagem nova no lugar da antiga. Imagem desenhada mais de uma vez, dentro de
  formulários (XObject de formulário) ou embutida no conteúdo (`BI…EI`): editar é recusado com aviso.
- Desfazer por instantâneo; Salvar grava; PDF assinado recusa; exige a permissão de alterar.

## 3. v4b — Texto

- Clique num bloco de texto abre a edição no lugar (o texto do bloco, quebras de linha mantidas,
  com a fonte e o tamanho do bloco); confirmar troca o bloco: o texto antigo sai pela redação (só
  texto, sem tarja) e o novo é escrito na mesma posição, com quebra automática na largura do bloco,
  na fonte padrão equivalente (Helvetica/Times/Courier, negrito/itálico conforme a original) e na
  mesma cor. **Adicionar texto** cria um bloco novo.
- Limitação conhecida: a fonte original subconjunto não é reaproveitada (troca pela equivalente
  padrão, como o Acrobat faz quando a fonte não está no sistema).

## 4. Testes

- v4a: mover muda a posição da imagem e mantém o objeto; redimensionar; girar; excluir; substituir;
  adicionar; imagem repetida recusa; outra imagem e o texto ficam intactos; desfazer.
- v4b: editar troca o texto e mantém o resto; acentos; quebra de linha; desfazer.
