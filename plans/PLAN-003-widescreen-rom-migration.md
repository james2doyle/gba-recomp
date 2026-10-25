# PLAN-003 — Migração ROM-only para apresentação ampliada

Contrato: [SPEC-070](../specs/SPEC-070-rom-only-widescreen-migration.md). Mecanismo: [ADR-043](../adr/ADR-043-rom-patch-at-load.md). Procedimento para o agente: [docs/widescreen-migration.md](../docs/widescreen-migration.md). Precedente: [PLAN-002](PLAN-002-widescreen.md).

## Estado

| Passo | Estado | Evidência |
|---|---|---|
| 1. Perfil v1 (`gba-profile`) | **DONE** | `cargo test -p gba-profile` 21 verdes; perfil de leafgreen do repositório carrega |
| 2. Teto por jogo ligado ao frontend | **DONE** | Rkanoid: sem perfil 240, perfil 288, `--view-width 320` vence; hash canônico idêntico |
| 3. Patches verificados na carga | **DONE** | `expected` divergente aborta com exit 1 e imagem intacta; patch real muda ciclos do guest |
| 4. Diagnóstico de margem | **DONE** | `gba-run --margin-report`: BG0 texto mapa 256; margem ring 48 / authored 32 / backdrop 0 em 320 px (simétrica após a correção do classificador, `gba105-review-fixes-20261024-linux-x86_64.json`) |
| 5. Guia de migração | **DONE** | `docs/widescreen-migration.md` |
| 6. Primeiro título-alvo | **BLOQUEADO** | Falta um título cujo diagnóstico dê `authored` majoritário |

Os passos 1–5 estão implementados e medidos. O passo 6 é o que falta para GBA-110 sair de BACKLOG, e **não depende de código**: depende de um alvo.

## Sequência

### G-A — Perfil v1 com `presentation` e `[[patch]]`
Concluído. Parser estrito, sem scripts, bytes literais, SHA-256 obrigatório, offsets disjuntos, `expected` byte-exato antes de escrever.
Aceite: A-70-01..07, A-70-11.

### G-B — Teto por jogo respeitando precedência
Concluído. `find_profile` por SHA exato; `effective_view_width` inalterado, então `--view-width` continua vencendo e o override continua não sendo persistido.
Aceite: A-70-02, A-70-03, A-70-04.

### G-C — Patches aplicados alto e cedo
Concluído nos três binários, antes de `Gba::new`, com log obrigatório de offset/escopo/motivo/evidência.
Aceite: A-70-05, A-70-06, A-70-10.

### G-D — Diagnóstico estrutural de margem
Concluído. Classificação pela camada vencedora, não por cor. O relatório também recusa um teto que a medição não sustenta.
Aceite: A-70-08, A-70-09.

### G-E — Guia e correção de registros
Concluído. O guia ordena inventário → portão de viabilidade → patch → verificação → registro, e registra as cinco armadilhas pagas.
Aceite: documentação coerente e links válidos.

### G-F — Primeiro título-alvo (bloqueia GBA-110)
**Não iniciado.** Requer um título cujo `--margin-report` dê `authored` majoritário — isto é, BG affine com mapa grande, ou BG de texto com mapa 512 que o jogo realmente use.

Candidatos a testar, em ordem de probabilidade:

1. Títulos com mundo rotativo/escalado (affine): corrida, shoot-em-up vertical, RPG com batalhas em perspectiva.
2. Títulos com mapa de texto 512 (verificar `BGxCNT` size 1 ou 3).
3. Títulos com HUD separado em BG próprio (patch de âncora isolado, sem tocar no mundo).

Critério de conclusão: perfil em `profiles/` com `status = "draft"`, medição em `knowledge/evidence/` e um `frame_hash` registrado antes/depois dos patches. Sem isso, GBA-110 continua sem critério de aceite.

## O que este plano NÃO faz

- Não cria conteúdo de margem. Patch corrige enquadramento; mapa de 256 continua 256.
- Não recompila o jogo. Caminho "source-available" é outro trabalho.
- Não substitui instruções no emissor LLVM. Fica adiado por ADR-043.
- Não distribui ROM alterada: os patches são aplicados em memória na máquina do usuário.

## Correção registrada

A medição anterior da margem do Rkanoid ("11716/12800 px de backdrop") estava **errada por circularidade**: comparava colunas da margem com a borda do próprio recorte de onde derivavam. A medição correta, feita dentro da superfície ampliada, dá **ring 64 / authored 16 / backdrop 0** (WS-32 em [KNO-011](../knowledge/KNO-011-widescreen-survey.md)). Consequência: o teto correto do Rkanoid é **240**, não 288. O roadmap foi corrigido junto.
