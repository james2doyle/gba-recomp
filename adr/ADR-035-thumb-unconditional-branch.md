# ADR-035 — Representar branch incondicional Thumb como alvo estático

- Estado: Proposed
- Data: 2026-10-14
- Supersede: —

## Contexto

A análise estática O2 da LeafGreen fixada em KNO-004 alcançou o halfword `0xE00A` em `0x08000AE6`. Os campos inferem Thumb `B` Format 18 com alvo `0x08000AFE`; o erro atual interrompe discovery antes da emissão. A fonte mGBA no commit fixado corrobora a extensão signed de `imm11` e o uso do PC Thumb, sem constituir execução da ROM.

## Decisão proposta

Adotar [SPEC-050](../specs/SPEC-050-thumb-unconditional-branch.md). Representar a instrução como uma operação de IR com um único destino estático constante. Discovery enfileira somente esse destino, mantendo o estado guest. A validação IR exige que o destino exista como instrução Thumb no programa. LLVM emite a saída direta ao dispatcher existente.

Manter `GBA_IR_VERSION` e ABI/runtime inalterados. Não usar a máscara de sucessores condicionais nem `StaticBranchTarget`, reservada a BX. Nenhum fallthrough é associado ao branch; um endereço sequencial só pode entrar no programa se outra aresta o alcançar.

## Alternativas

- Modelar B como branch condicional com edge único: rejeitada porque exige CPSR e inclui predicação inexistente no encoding.
- Manter o fallthrough no CFG por conveniência: rejeitada porque alarga o conjunto de código exigido pelo contrato sem representar uma aresta guest.
- Implementar BL ou alterar BX junto: rejeitada para manter o incremento pequeno, rastreável e verificável.
- Fallback para interpretação/JIT: rejeitada pelo requisito strict-AOT.

## Consequências

O branch resolve diretamente destinos estaticamente codificados, sem novo estado ou custo de runtime. Um destino fora da imagem ou que contenha instrução ainda não suportada mantém a compilação estrita em erro, sem pacote parcial. O trabalho não representa branches Thumb em geral nem garante compatibilidade de LeafGreen.

## Referências

- [SPEC-050](../specs/SPEC-050-thumb-unconditional-branch.md).
- [SPEC-032](../specs/SPEC-032-thumb-conditional-branch.md).
- [SPEC-018](../specs/SPEC-018-arm-thumb-branch-exchange.md).
- [SRC-44](../knowledge/KNO-001-evidence.md).
