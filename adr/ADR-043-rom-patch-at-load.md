# ADR-043 — Patches de ROM aplicados na carga, não hooks no codegen

- Estado: Proposed
- Data: 2026-10-24
- Contrato: [SPEC-070](../specs/SPEC-070-rom-only-widescreen-migration.md)
- Complementa: [ADR-042](ADR-042-widescreen-presentation-surface.md), [ADR-041](ADR-041-interpreter-first-core.md)

## Contexto

[SPEC-069](../specs/SPEC-069-widescreen-presentation.md) entrega a superfície ampliada, mas um título só recebe margem se declarar um teto, e isso exige um perfil por SHA-256. Para um jogo **sem código-fonte**, corrigir enquadramento (limite de câmera, âncora de HUD, culling de sprites) implica alterar o comportamento do guest em pontos específicos.

O repositório hoje não tem nenhum mecanismo de patch: uma busca por substituição de instrução em `crates/` não encontra nada, e `gba-llvm` emite blocos a partir do IR sem qualquer noção de "reescrever este PC". O `Profile` em `crates/gba-recomp/src/compat.rs` é outro conceito — resultado de uma execução de descoberta, não o perfil de SPEC-008.

Existem três lugares onde a alteração poderia viver.

## Decisão

**Patches de bytes na imagem de ROM em memória, antes de `Gba::new`.**

A imagem lida do disco é copiada, verificada por SHA-256 contra o perfil, e então cada patch é aplicado verificando `expected` byte a byte. O resultado é passado ao runtime exatamente como se fosse o ROM original.

Consequências diretas:

- Funciona **identicamente** no interpretador, em `aot-compatible` e em `strict-aot`, porque todos consomem a mesma imagem.
- Não exige nenhuma facilidade nova no emissor LLVM nem no despachante nativo.
- É auditável: o perfil contém offset, bytes esperados, bytes novos, motivo e evidência; o log de execução imprime todos.
- É reproduzível: mesma imagem + mesmo perfil = mesmos bytes.

## Alternativas

**A. Substituição de instrução no emissor (`gba-llvm`), por PC.** Rejeitada **por ora**. Reimplementar a semântica de uma instrução exige que o emissor conheça o mapa de funções e o IR daquele PC, e o resultado deixaria de ser verificável byte a byte — a auditoria passaria a depender de comparar comportamento. Também cria dois caminhos: o interpretador não passaria pelo hook, então `--verify-interpreter` divergiria por construção. Fica registrada como evolução possível quando existir um registro de hooks com ABI declarada (PROF-03); o custo é ordens de grandeza maior que o de um patch de bytes.

**B. Recompilar o jogo a partir do código-fonte.** Fora de escopo: o alvo desta decisão é um título que só existe como ROM. É o caminho "source-available", onde o desenvolvedor edita o layout (mapa 512, HUD reposicionado) e o host apenas respeita o teto declarado.

**C. Deixar o host inventar margem.** Rejeitada pela [SPEC-070](../specs/SPEC-070-rom-only-widescreen-migration.md) (§Limite duro) e por [ADR-042](ADR-042-widescreen-presentation-surface.md) §5: margem vem de amostragem de hardware ou é backdrop/anel, nunca conteúdo sintetizado.

**D. Não fazer nada e ficar em 240.** É o default e continua válido. A decisão existe para permitir sair dele com evidência, não para obrigar.

## Riscos

| Risco | Mitigação |
|---|---|
| **Patch desloca o comportamento do guest.** Não é um ajuste de apresentação: o jogo passa a executar outra instrução, e o hash de frame muda | `expected` byte-exato antes de aplicar, hash do frame registrado antes/depois no cenário nomeado, e a regra de que o patch vive no perfil com evidência — nunca escondido em código |
| Patch em código copiado para IWRAM não alcança a cópia | Limitação conhecida e declarada; detectável apenas em runtime, registrada como questão aberta da SPEC-070 |
| Perfil de outra revisão aplicado "por proximidade" | PATCH-70-02/PATCH-70-10: só SHA exato; revisão diferente é ignorada com diagnóstico |
| Patches viram dívida invisível | Todo patch tem `reason`, `evidence`, `affects`, e o log imprime os três na aplicação |
| Leitura ingênua de que a margem virou conteúdo | A-70-08 condiciona o teto ao `--margin-report` classificar a margem como `authored`; caso contrário o teto é rejeitado |

## Consequências

- O perfil passa a ser um artefato executável de compatibilidade, não só um registro de intenção. Isso aumenta o valor e também a responsabilidade: um perfil com patch errado muda o jogo.
- O digest de código do pacote (PROF-05) precisa incluir patches e `view_ceiling`.
- `--view-width` continua vencendo o teto (WS-69-18), então uma sessão experimental não é bloqueada por perfil ausente.
- Nenhuma evidência existente é invalidada: sem perfil, o comportamento é bit-idêntico ao de hoje.

## Critérios para revisitar

- Se um título exigir mais de ~10 patches, ou patches em laços copiados para IWRAM, o custo do caminho de bytes passa a competir com o hook por PC e a alternativa A deve ser reavaliada.
- Se o primeiro alvo tiver mapa de texto 256 e a margem desejada não for anel/backdrop, o caminho ROM-only não resolve — será preciso reavaliar o escopo da SPEC-070 em vez de forçar o teto.
- Se aparecer patch que depende de estado de runtime (modo de vídeo, nível), a v1 sem condição se mostra insuficiente e a SPEC-070 precisa de condição declarativa.

## Referências

[SPEC-070](../specs/SPEC-070-rom-only-widescreen-migration.md), [SPEC-008](../specs/SPEC-008-game-profiles.md) (PROF-01..08), [SPEC-069](../specs/SPEC-069-widescreen-presentation.md), [ADR-042](ADR-042-widescreen-presentation-surface.md), [KNO-011](../knowledge/KNO-011-widescreen-survey.md).
