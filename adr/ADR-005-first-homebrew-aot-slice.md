# ADR-005 — Dispatcher AOT da primeira homebrew

- Estado: Proposed
- Data: 2026-10-13
- Supersede: —

## Contexto

GBA-010 provou emissão LLVM e ABI C com uma instrução isolada, mas não exercitou intake de uma ROM nem um executável completo. O runtime de hardware ainda não existe. GBA-020 precisa fechar um caminho estrito e reproduzível sem declarar compatibilidade que a implementação não tem.

## Decisão proposta

Usar uma homebrew curta gerada pelo próprio repositório. O frontend valida o header e o SHA-256, descobre apenas branches ARM diretos e recusa qualquer instrução alcançável fora do subconjunto declarado. LLVM emite um dispatcher nativo com casos estáticos por PC; o runner chama a ABI C v1 com orçamento explícito de instruções. Esse slice não inclui interpretador, JIT, memória guest ou periféricos. `guest_cycle` avança uma unidade abstrata por instrução e não deve ser usado como timing ARM7TDMI.

## Alternativas

- Manter somente `emit-fixture` demonstraria o lowerer, mas não ROM→executável.
- Tratar opcode desconhecido como NOP ou reinterpretá-lo ocultaria lacunas e alteraria a semântica guest.
- Incluir o runtime/interpreter da referência aceleraria cobertura, mas contrariaria o escopo clean-room registrado e exigiria proveniência, avisos e revisão de licença por módulo.

## Consequências

O caminho completo é verificável com uma ROM de 256 bytes e executável host pequeno. O subset não pode executar jogos comerciais; a primeira compilação de LeafGreen já falha em `0x08000208`, sem produzir pacote. CPU memory-mapped, modos Thumb, BIOS, scheduler, PPU/APU e input são contratos futuros. A evidência O0/O2 aplica-se somente à Tiny Counter e à janela de 100 instruções.

## Critérios para revisitar

Revisitar quando GBA-025/030 adicionarem CFG e bus/scheduler: substituir o dispatcher por bloco ou shard só após preservar guards, checkpoints, eventos, misses explícitos e equivalência O0/O2. Timing passa a usar SPEC-005, nunca a unidade sintética deste slice.

## Referências

[SPEC-012](../specs/SPEC-012-first-homebrew.md), [ROADMAP GBA-020](../ROADMAP.md), [evidência local](../knowledge/evidence/gba020-macos-arm64.json), [KNO-004](../knowledge/KNO-004-rom-intake.md).
