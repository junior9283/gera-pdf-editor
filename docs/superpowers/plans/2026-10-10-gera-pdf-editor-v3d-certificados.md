# Certificados (v3d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Assinatura digital PAdES básica (adbe.pkcs7.detached) e validação, como o Acrobat.

**Architecture:** pdf-core lê/cria IDs (PKCS#12), assina (gravação incremental + CMS) e valida; app-state guarda listas de IDs e de confiança; gera-ui tem a linha, as janelas, o painel e a barra.

**Tech Stack:** Rust, mupdf 0.8, RustCrypto (cms 0.2, x509-cert 0.2, rsa 0.9, sha2, p256), p12-keystore.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v3d-certificados-design.md

## Global Constraints

- Licenças MIT/Apache/BSD (compatíveis com AGPL). Senhas nunca gravadas nem registradas.
- Salvar documento assinado: sempre incremental.

## Review Focus

- PDF malicioso: `/ByteRange` fora do arquivo, negativo, sobreposto; `/Contents` não-hex ou enorme; CMS malformado — validar sem pânico.
- Assinar documento com alterações pendentes ou protegido por senha.
- Arquivo aberto em outra aba/programa mudado no disco antes de assinar.
- Senha errada, ID apagado, PKCS#12 sem chave ou com chave não-RSA/ECDSA.
- Muitas assinaturas (10+) e revisões: validação não fica quadrática no tamanho do arquivo.

---

### Task 1: pdf-core — IDs digitais (ler e criar PKCS#12)
### Task 2: pdf-core — assinar (incremental, ByteRange, CMS)
### Task 3: pdf-core — validar e Salvar incremental
### Task 4: app-state + worker
### Task 5: gera-ui — linha, janelas, painel e barra
### Task 6: E2E, simplify, medição, revisão, merge
