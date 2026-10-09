#!/usr/bin/env bash
# Mede as metas de leveza do spec (§1). Precisa de uma sessão gráfica (abre uma janela por instantes).
# Uso: scripts/measure.sh [arquivo.pdf]   (sem argumento, usa um PDF sintético de 100 páginas)
set -euo pipefail
cd "$(dirname "$0")/.."

MAX_BIN=$((25 * 1024 * 1024))
MAX_MS=300
MAX_RSS_KB=$((150 * 1024))
RUNS=5

cargo build --release -p gera-ui -q
BIN=target/release/gera-pdf-editor
SIZE=$(stat -c%s "$BIN")

if [ -n "${1:-}" ]; then
  SAMPLE=$1; LABEL="PDF real"
else
  SAMPLE=$(mktemp --suffix=.pdf); LABEL="PDF sintético (texto simples: melhor caso)"
  trap 'rm -f "$SAMPLE"' EXIT
  cargo run --release -q -p pdf-core --example make_sample -- "$SAMPLE" 100
fi
echo "amostra: $LABEL"

# executa o app em modo de medição; sem resposta em 60 s a sessão gráfica provavelmente está bloqueada
bench() {
  timeout 60 "$BIN" "$@" "$SAMPLE" || { echo "FALHA: o app não respondeu em 60 s (sessão gráfica ativa e desbloqueada?)" >&2; exit 1; }
}
field() { grep -o "$1=[0-9]*" <<<"$2" | cut -d= -f2; } # extrai um campo numérico da saída BENCH
median() { printf '%s\n' "$@" | sort -n | sed -n "$((($# + 1) / 2))p"; }

# a primeira execução aquece o cache de disco; vale a mediana das seguintes
bench --bench-open >/dev/null
MS_LIST=(); RSS_LIST=()
for _ in $(seq "$RUNS"); do
  OUT=$(bench --bench-open | grep '^BENCH')
  MS_LIST+=("$(field open_ms "$OUT")"); RSS_LIST+=("$(field rss_kb "$OUT")")
done
MS=$(median "${MS_LIST[@]}"); RSS=$(median "${RSS_LIST[@]}")

# percurso por todas as páginas: a memória não pode crescer sem limite ao rolar
SCROLL=$(bench --bench-scroll | grep '^BENCH scroll')
RSS_SCROLL=$(field rss_kb "$SCROLL")

echo "binario_bytes=$SIZE (limite $MAX_BIN)"
echo "open_ms=${MS_LIST[*]} (mediana $MS, limite $MAX_MS)"
echo "rss_kb=${RSS_LIST[*]} (mediana $RSS, limite $MAX_RSS_KB)"
echo "$SCROLL (limite rss_kb<$MAX_RSS_KB)"

FAIL=0
# valor ausente (medição que falhou) conta como falha
check() { [ "${2:-}" -lt "$3" ] 2>/dev/null || { echo "FALHA: $1 = '${2:-?}' (limite $3)"; FAIL=1; }; }
check "tamanho do binário" "$SIZE" "$MAX_BIN"
check "abertura (ms)" "$MS" "$MAX_MS"
check "memória na abertura (kB)" "$RSS" "$MAX_RSS_KB"
check "memória após rolar (kB)" "$RSS_SCROLL" "$MAX_RSS_KB"
exit $FAIL
