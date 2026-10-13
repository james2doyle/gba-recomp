# ADR-006 — ABI v2 para estado ARM bancado

- Estado: Proposed
- Data: 2026-10-13
- Supersede: —

## Contexto

O ABI v1 mantém apenas r0–r15/CPSR/ciclo e é suficiente para a fixture ALU. A ROM fornecida alcança `MSR CPSR`, que pode trocar o modo CPU e os bancos de SP/LR. Alterar o layout v1 silenciosamente invalidaria seu contrato e confundiria os dois geradores.

## Decisão proposta

Manter ABI v1 imutável e adicionar ABI v2 com contexto bancado de 192 bytes. O dispatcher compilado para jogos usa `gba_block_v2`; o pacote inclui um helper C pequeno para aplicar `MSR CPSR` com validação atômica e alternância de bancos. O helper é parte do runtime guest ABI, linkado estaticamente; endereços guest continuam inteiros. Até Thumb estar compilado, alteração do bit T retorna `missing_code` sem mutação.

## Alternativas

- Ampliar ABI v1 quebraria os offsets publicados em SPEC-004 e a fixture existente.
- Descartar os bancos ao trocar modo deixaria SP/LR incorretos e poderia corromper stacks de exceção.
- Simular `MSR` diretamente no host em código gerado sem helper duplicaria lógica de banco em cada instrução e tornaria a verificação difícil.

## Consequências

Há dois contratos de ABI e cada objeto deve declarar um. Pacotes v2 incluem um helper C e testam layout em C/Rust/LLVM. A mudança de modo torna o primeiro caminho real da ROM mais próximo, mas o jogo continua sem suporte a loads/stores, Thumb, BIOS, bus e vídeo.

## Critérios para revisitar

Revisitar o helper quando exceções/IRQ e Thumb forem implementados. O contrato deverá cobrir retorno por SPSR, transição ARM↔Thumb e materialização do PC/pipeline antes de promover a cobertura além do boot inicial.

## Referências

[SPEC-013](../specs/SPEC-013-banked-cpu-and-psr.md), [SPEC-002](../specs/SPEC-002-armv4t-semantics.md), [SPEC-004](../specs/SPEC-004-ir-llvm.md), [SRC-11/KNO-001](../knowledge/KNO-001-evidence.md).
