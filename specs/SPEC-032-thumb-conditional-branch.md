# SPEC-032 — Branch condicional Thumb Format 16 em AOT estrito

Estado do documento: Accepted. Implementada no commit `eabe929`; evidência: [gate GBA-020](../knowledge/evidence/gba020-thumb-conditional-branch-macos-arm64.json). Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-031, a descoberta estática da ROM LeafGreen fixada em KNO-004 parou em `0x080009A6`, raw `0xDDF6`. Os campos inferem `BLE` com imm8 `0xF6` (−10 halfwords). O PC visível Thumb é `guest_pc+4`, resultando no alvo `0x08000996`; o caminho sequencial é `0x080009A8`. SRC-33 registra as observações nas fontes fixadas. Nenhum NZCV, caminho tomado ou execução dinâmica da ROM foi observado.

Esta tranche aceita somente o branch condicional Thumb Format 16 para condição `0x0..=0xD`. Não inclui branch incondicional Format 18, BL, SWI, outras instruções da família Thumb, nem inferência ou propagação de flags na análise estática.

## Requisitos normativos

- TBC-01: o decoder MUST reconhecer Format 16 quando `(encoding & 0xF000) == 0xD000` e `condition = bits[11:8]` estiver em `0x0..=0xD`. Deve preservar `guest_pc`, encoding, condition e destino calculado por `guest_pc.wrapping_add(4).wrapping_add(sign_extend(imm8) << 1)`, com `imm8 = bits[7:0]` assinado e deslocado à esquerda um bit. A aritmética de PC MUST ter wrap módulo 2^32. `condition=0xE` MUST continuar `undefined_thumb_instruction`; `condition=0xF` MUST continuar sob o decoder SWI Thumb existente.
- TBC-02: durante execução, a condição MUST ser avaliada pelos bits NZCV do CPSR conforme ARMv4T. O estado passa a `PC=target` se satisfeita ou `PC=guest_pc.wrapping_add(2)` caso contrário. `r0–r14`, CPSR/NZCV, modo, memória e estado de hardware MUST permanecer inalterados; `r15/PC` é atualizado pelo resultado da condição. O ciclo abstrato MUST avançar em uma unidade em ambos os resultados.
- TBC-03: a GBA IR MUST representar o branch condicional sem usar `StaticBranchTarget`, que permanece reservado a `BX`. O verificador MUST confirmar que a variante, condição e destino correspondem ao encoding, condição está em `0x0..=0xD`, PC de origem/alvo/fallthrough estão alinhados a halfword e tanto o destino quanto `guest_pc.wrapping_add(2)` existem como instruções Thumb no programa. Campo inconsistente ou sucessor ausente MUST ser rejeitado.
- TBC-04: discovery MUST enfileirar alvo e fallthrough com o mesmo estado abstrato de registradores. Não pode avaliar NZCV, escolher um caminho nem remover um predecessor com base em flags. Ambos os caminhos MUST ser decodificados e cobertos por strict-AOT antes da publicação do pacote.
- TBC-05: LLVM MUST emitir avaliação nativa da condição e seleção entre PCs constantes de destino e fallthrough. A operação MUST usar o dispatcher pelo par `(modo, PC)` existente; nenhum fallback a interpretador ou JIT é permitido.
- TBC-06: `GBA_IR_VERSION`, layouts de ABI/runtime e a semântica ARM/Thumb existente MUST permanecer inalterados. A alteração não muda os requisitos BX de SPEC-018 nem amplia a semântica de branches ARM.
- TBC-07: alcançar instrução não suportada em qualquer sucessor MUST falhar antes de publicar pacote parcial, conforme o modo strict-AOT da SPEC-002/SPEC-003.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| Condições `0x0..=0xD` | Decoder preserva os 14 códigos válidos e seus immediatos signed |
| `0xDDF6` em `0x080009A6` | Condition LE, deslocamento −20 bytes, alvo `0x08000996`, fallthrough `0x080009A8` |
| Imediato negativo/zero/positivo e wrap de PC | Alvo módulo 2^32, alinhado a halfword |
| `DExx` / `DFxx` | Condição `0xE` continua Undefined; `0xF` continua SWI |
| LE satisfeita | `Z=1` ou `N!=V` seleciona o alvo |
| LE não satisfeita | `Z=0` e `N=V` seleciona `PC+2` |
| Todas as condições | `execute_thumb_instruction` concorda com a tabela de condição para as 16 combinações NZCV |
| Estado e ciclo | r0–r14 e CPSR preservados; PC escolhido corretamente; ciclo +1 nos dois caminhos |
| Discovery e verificador IR | Alvo e fallthrough são exigidos no programa com estado abstrato de registradores não condicionado por flags |
| O0/O2 | A `ArmProgram` verificada contém entradas para target e fallthrough; o manifesto confirma strict-AOT e a execução das fixtures escolhe cada caminho conforme NZCV |
| Sucessor alvo não compilável | Uma instrução não suportada no sucessor alvo falha reportando o PC dessa instrução e não publica pacote parcial |
| Sucessor fallthrough não compilável | Uma instrução não suportada no sucessor fallthrough falha reportando o PC dessa instrução e não publica pacote parcial |

## Fixtures e verificações

Adicionar casos de decoder para todos os códigos de condição, incluindo immediatos `0x00`, `0x7F`, `0x80` e `0xFF`, alvo `0xDDF6`, wrap do PC e as classificações preservadas de DExx/DFxx. Os testes de IR devem cobrir tomada/não tomada, todos os códigos de condição contra todas as combinações NZCV, preservação de r0–r14/CPSR, atualização de PC, ciclo, encoding inconsistente e remoção independente do alvo e fallthrough. Discovery deve comprovar que ambos os destinos são enfileirados com o mesmo estado independente de flags. As fixtures de pacote devem exercitar ambos os resultados em O0/O2, confirmar no programa verificado a presença dos dois sucessores, checar separadamente o manifesto strict-AOT e rejeitar compilação quando tanto o alvo quanto o fallthrough tenham, em casos distintos, instruções ainda não aceitas. Cada falha deve apontar o PC da instrução não suportada e deixar o pacote ausente. Após o gate, repetir a descoberta estática de LeafGreen e registrar o próximo limite alcançado.

Fixtures sintéticas validam somente o contrato e seus estados iniciais; não demonstram qual caminho LeafGreen toma nem execução em hardware.

## Relação com specs e referências

SPEC-032 amplia somente a família condicional direta de Thumb e supersede a afirmação de escopo em SPEC-018 de que o decoder Thumb reconhecia apenas BX. BX-01 a BX-08 e a necessidade de fechar alvos indiretos continuam inalterados. `CFG-01`, `CFG-06` e `CFG-08` em SPEC-003 continuam aplicáveis. `CPU-01`, `CPU-03` e `CPU-04` em SPEC-002 seguem os contratos de alinhamento, PC, flags e predicação. SRC-33 em [KNO-001](../knowledge/KNO-001-evidence.md) registra fontes fixadas de decode e condição; [KNO-004](../knowledge/KNO-004-rom-intake.md) fixa ROM, hash e fronteira estática. As fontes não provam NZCV nem o caminho executado pelo jogo.
