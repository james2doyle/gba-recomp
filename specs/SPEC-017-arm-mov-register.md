# SPEC-017 — MOV ARM entre registradores sem shift

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar a forma ARM `MOV Rd, Rm` com o shifter codificado como `LSL #0`, sem atualização de flags. `Rm=PC` lê o PC visível do pipeline (`guest_pc+8`). Escrita em PC, shifts diferentes e as demais operações ALU continuam fora deste incremento.

## Requisitos normativos

- MOVREG-01: aceitar somente data processing ARM com `I=0`, opcode `MOV`, `S=0`, campo `Rn=0`, `bit4=0`, shift `LSL`, shift amount zero e `Rd=r0-r14`. `Rm` pode ser r0-r15. A condição deve ser definida (0–14).
- MOVREG-02: se `Rm` for r0-r14, ler o valor guest desse registrador. Se `Rm=PC`, usar `guest_pc+8` em aritmética módulo 2^32. Esta forma é um valor de operando, não controle de fluxo.
- MOVREG-03: se a condição passar, copiar o valor para `Rd`, sem alterar CPSR. Se falhar, preservar `Rd` e CPSR. Ambos os caminhos avançam PC de controle em quatro bytes e incrementam uma unidade de ciclo abstrato.
- MOVREG-04: o lowering AOT não pode ler um PC host nem converter endereços guest em ponteiros. A execução permanece estritamente AOT e não ganha fallback de interpretação/JIT.
- MOVREG-05: `MOVS`, qualquer shift diferente de `LSL #0`, forma por shift registrado e destino PC continuam `unsupported_instruction` nesta spec.

## Critérios de aceite

| Caso | Resultado esperado |
|---|---|
| `MOV LR,PC`, seguido de `MOV r0,LR` | `r0=guest_pc_inicial+8`; mesmo estado final em O0/O2 |
| `MOV Rd,Rm` com `Rm` geral | Copia o valor de origem; flags preservadas |
| Condição falsa | Destino/CPSR preservados; PC e ciclo avançam |
| `MOVS`, shift não nulo, shift por registrador ou destino PC | Compilação recusada sem pacote parcial |
| Todos os casos | Manifesto continua `strict-aot`; nenhum código de interpretação/JIT |

## Evidência e limites

A descoberta AOT local parou em `0x0800022C`, encoding `0xE1A0E00F`; `MOV LR,PC` é inferência pelos campos. SRC-18 registra observações estáticas de mGBA e gba-recomp sobre o shifter e PC. A spec cobre apenas esta forma; não afirma que a instrução seja executada pelo jogo, nem que LeafGreen compile ou inicialize.
