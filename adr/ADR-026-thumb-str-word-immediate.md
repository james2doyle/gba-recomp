# ADR-026 — STR word Thumb usando helper ABI v3

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

Após SPEC-038, a descoberta estática de LeafGreen alcançou `0x080006B4`, raw `0x6005`, inferido como Thumb `STR r5,[r0,#0]`. A ABI v3 já expõe `gba_store32_v3`, que grava word little-endian em EWRAM/IWRAM físicas e falha em desalinhamento, ABI inválida ou região fora do subset.

## Decisão

Adicionar a operação interna de IR `StoreWordImmediate` e lowering Thumb que calcula `Rb + imm5*4` em u32 e chama uma vez `gba_store32_v3`. Capturar base e valor antes da chamada para cobrir `Rd==Rb`; commitar PC/ciclo somente após sucesso. Discovery invalida a word sobreposta da sombra, sem afirmar o valor escrito.

Reutilizar ABI v3 evita duplicar validação e mantém EWRAM/IWRAM como única região suportada nesta tranche. MMIO e DMA continuam gerando fault runtime explícito; compilar a operação não declara que a ROM possa atravessar esse acesso.

## Alternativas

- Criar novo helper ABI para Thumb: rejeitada porque o efeito é o mesmo STR word já coberto por `gba_store32_v3`.
- Converter endereço guest em ponteiro LLVM/host: rejeitada porque contorna validação e quebra a separação guest/host.
- Ignorar store em região não modelada ou usar interpretação/JIT: rejeitada pelo modo strict-AOT.

## Consequências

Não há mudança de layout ou versão ABI, nem mudança do GBA IR v2. A nova enum variant é interna e o `ArmProgram` não é serializado. Fixtures O0/O2 devem exercitar a operação nova através do runtime v4 e helper v3.

## Critérios para revisitar

- Um jogo exigir acesso word dinâmico a MMIO/DMA; isso requer contrato de bus independente, não ampliar `gba_store32_v3` silenciosamente.
- Evidência de hardware/oráculo divergir na política de endereços desalinhados.

## Referências

- [SPEC-039 — Thumb STR word imediato](../specs/SPEC-039-thumb-str-word-immediate.md)
- [SPEC-016 — STR ARM e helper word-store](../specs/SPEC-016-ram-word-stores.md)
- [SPEC-034 — memória abstrata](../specs/SPEC-034-abstract-memory-discovery.md)
- [KNO-004 — LeafGreen fixada](../knowledge/KNO-004-rom-intake.md)
