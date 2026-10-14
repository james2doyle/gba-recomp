# SPEC-041 — LSR Thumb imediato Format 1 em AOT estrito

Estado do documento: Accepted. Implementação e evidência: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-040, a descoberta estática da ROM LeafGreen fixada em KNO-004 alcançou `0x08000B80`, halfword `0x0C00`, e parou com `unsupported_thumb_instruction`. Os campos inferem Thumb Format 1 `LSR r0,r0,#16`; o próximo halfword observado em `0x08000B82` é `0x4C07`. A classificação é inferência estática, não execução guest.

Adicionar LSR imediato Thumb T1 ao subset strict-AOT. É uma operação de CPU sem acesso a memória guest, ABI/runtime auxiliar ou dependência de periféricos.

## Fora de escopo

- ASR imediato, shifts por registrador e demais operações ALU não listadas.
- Mudança da ABI, estado de runtime, GBA IR v2, manifesto, interpretação/JIT ou fallback.
- Suporte ou alegação de compatibilidade com LeafGreen.

## Invariantes

- Format 1 LSR: `(encoding & 0xF800) == 0x0800`; `imm5=bits[10:6]`, `Rs=bits[5:3]`, `Rd=bits[2:0]`, todos r0–r7.
- Para `imm5` entre 1 e 31, `result = old_Rs >> imm5` e `C = old_Rs[imm5-1]`. Para `imm5==0`, a codificação significa deslocamento de 32: `result=0` e `C=old_Rs[31]`.
- A instrução grava `Rd` usando o valor prévio de Rs, atualiza N/Z/C, preserva V e os demais bits de CPSR, avança Thumb PC em dois bytes e soma um ciclo abstrato. `Rd==Rs` é permitido.
- A descoberta propaga valores singleton/conjuntos finitos exatamente; Unknown continua Unknown. Não precisa de estado de memória.

## Requisitos normativos

- TLS-01: decoder e verificador MUST aceitar somente o encoding Format 1 `0x0800`, conferir `Rd`, `Rs` e `imm5` contra a palavra e rejeitar registradores fora de r0–r7.
- TLS-02: `ThumbOperation::LsrImmediate { rd, rs, amount }` MUST implementar os resultados e carry definidos acima, inclusive `amount==0` como shift de 32, sem usar shift LLVM inválido ou poison.
- TLS-03: o lowering MUST capturar o valor antigo de Rs, atualizar somente Rd e as flags N/Z/C, preservar V e demais bits de CPSR, então avançar PC+2 e ciclo+1. Semântica deve ser idêntica em O0/O2.
- TLS-04: single-step deve implementar a mesma semântica. Descoberta MUST aplicar shift exato aos estados constantes, preservar os outros registradores, e enfileirar `PC+2`.
- TLS-05: nenhuma chamada helper, mudança ABI/layout ou fallback de interpretação/JIT será introduzida. O manifesto permanece `strict-aot`.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0x0C00` | Decodifica `LSR r0,r0,#16` |
| Imediato 0 | Resultado 0; carry vem do bit 31 da fonte |
| Imediatos 1, 16 e 31 | Shift lógico e carry usam os bits especificados |
| Fontes zero, bit 31, bit de carry de saída e padrão geral | Rd/N/Z/C corretos; V e bits não relacionados preservados |
| `Rd==Rs` | O shift usa snapshot da fonte antes de gravar destino |
| Discovery com singleton/conjunto e Unknown | Propaga cada constante exatamente; Unknown permanece Unknown |
| O0/O2 | Fixtures strict-AOT executam o mesmo resultado e estado |
| LSL imediato existente e ASR imediato | LSL mantém o comportamento existente; ASR permanece rejeitado |

Após fixtures passarem, repetir a compilação estática de LeafGreen e registrar somente a nova fronteira. Isso não demonstra execução nem compatibilidade do título.

## Fontes e referências

- [KNO-001 / SRC-39](../knowledge/KNO-001-evidence.md) fixa observações do decoder e da semântica de referência.
- [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa identidade, hashes e fronteiras estáticas da ROM.
- [SPEC-031](SPEC-031-thumb-immediate-format-3.md), [SPEC-037](SPEC-037-thumb-ldmia-rom-cases.md), [SPEC-038](SPEC-038-discovery-state-budget.md) e [SPEC-040](SPEC-040-thumb-ldr-word-immediate.md).
- [ADR-028](../adr/ADR-028-thumb-lsr-immediate.md) registra a decisão de manter LSR como operação IR explícita.
