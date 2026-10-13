# SPEC-015 — ADD imediato relativo ao PC em ARM

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Compilar a forma ARM `ADD Rd, PC, #imm` sem atualização de flags, para `Rd=r0-r14`. O caso materializa em registrador um endereço guest derivado do PC visível da instrução. Não lê memória, não muda o PC de controle e não converte endereços guest em ponteiros host.

## Requisitos normativos

- ADDPC-01: aceitar somente data processing ARM com `I=1`, opcode `ADD`, `S=0`, `Rn=r15` e `Rd<15`. Formas por registrador, escrita de flags, destino PC e outras operações continuam recusadas.
- ADDPC-02: decodificar `imm32` como `ROR(imm8, 2*rotate)` e usar `guest_pc+8` como valor arquitetural de `PC` nessa instrução. O resultado `PC+imm32` segue soma guest módulo 2^32.
- ADDPC-03: se a condição ARM passar, gravar o resultado em `Rd` sem alterar CPSR. Se falhar, preservar `Rd` e CPSR. Ambos os caminhos avançam PC de controle em quatro bytes e incrementam uma unidade de ciclo abstrato.
- ADDPC-04: endereço calculado permanece um inteiro guest. Nenhum lowering pode tratá-lo como ponteiro host ou acessar memória por ele.

## Critérios de aceite

| Caso | Resultado esperado |
|---|---|
| Fixture `ADD r0, pc, #0x20` | Valor `guest_pc+8+0x20`, com estado final igual em O0/O2 |
| Condição falsa | Destino e CPSR preservados; PC/ciclo avançam |
| Imediato com rotação ARM | Valor corresponde ao `ROR` de oito bits, sem flags |
| `S=1`, destino PC ou operando por registrador | `unsupported_instruction`; sem pacote parcial |
| Endereço resultante | Continua inteiro guest; nenhuma carga ou ponteiro host |

## Evidência e limites

O compilador observou `0xE28F0020` em `0x08000220` na ROM local após habilitar o literal ROM. A leitura como `ADD r0, pc, #0x20` e o resultado `0x08000248` são inferências pelos campos ARM e PC+8. SRC-13 registra a observação da fonte mGBA no commit fixado; isso não é validação em hardware nem evidência de execução do jogo.
