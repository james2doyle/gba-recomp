# SPEC-025 — Thumb LDR literal de ROM em AOT estrito

Estado do documento: Draft. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Após SPEC-024, a descoberta estática da ROM fixada em KNO-004 alcançou o halfword `0x4A2C` em `0x080003B4` e parou no decoder. Os campos classificam o encoding como Thumb Format 6 `LDR r2,[PC,#0xB0]`; `Align(0x080003B4+4,4)+0xB0` resulta em `0x08000468`. A classe e o endereço são inferências dos campos corroboradas por SRC-26. Esta observação não estabelece o conteúdo consultado em `0x08000468` nem execução da ROM.

O objetivo é aceitar somente Thumb LDR literal cujo word esteja dentro da imagem ROM imutável, fixar seu valor na GBA IR e materializá-lo em código nativo strict-AOT.

## Fora de escopo

Loads Thumb por registrador ou memória dinâmica, byte/halfword, LDM, bus guest, RAM, I/O, mirrors ou banking do cartucho, endereços fora da imagem, fallback interpretado/JIT e alegações de timing físico ficam fora deste contrato. SPEC-014 continua definindo separadamente a forma ARM de load literal.

## Termos e invariantes

- `ROM_BASE` é o início guest da imagem identificada pelo SHA-256.
- Endereços guest são inteiros. Nenhum endereço guest é convertido em ponteiro host.
- `RomImage` é a fonte imutável usada durante descoberta; seu leitor verifica alinhamento e limite de quatro bytes.
- A compilação continua estritamente AOT. Uma palavra válida de ROM pode virar constante, mas não vira código alcançável sem uma transferência de controle descoberta.

## Requisitos normativos

- TLDR-01: o decoder MUST aceitar somente encodings com `(encoding & 0xF800) == 0x4800`, extrair `Rd=bits[10:8]` e `imm8=bits[7:0]`, e rejeitar os demais formatos de load Thumb ainda não contratados.
- TLDR-02: o endereço guest MUST ser calculado com aritmética verificada como `Align(guest_pc + 4, 4) + imm8 * 4`. Overflow, PC inválido ou endereço fora do intervalo da imagem MUST resultar em erro de compilação que informe o PC da instrução e o endereço calculado quando disponível.
- TLDR-03: a descoberta MUST ler exatamente quatro bytes little-endian por `RomImage`, apenas se todo o intervalo `[address,address+4)` couber na imagem identificada. Não deve resolver cartucho mirroring nem consultar o bus runtime.
- TLDR-04: a forma ainda não resolvida da operação MUST ser distinta da forma resolvida na GBA IR. `ArmProgram::verify` MUST rejeitar operação Thumb não resolvida, PC Thumb desalinhado, Rd fora de r0–r7, endereço não alinhado ou campos de IR divergentes do encoding.
- TLDR-05: a forma resolvida MUST carregar o valor como inteiro em Rd, preservar integralmente CPSR e outros registradores, avançar PC em dois bytes e contabilizar uma unidade abstrata. Não define custo de waitstate do Game Pak.
- TLDR-06: a análise abstrata MUST propagar o valor da palavra para Rd e permitir que um `BX Rd` subsequente feche um alvo estático. A carga não MUST adicionar por si só endereço de código ao CFG.
- TLDR-07: o lowering LLVM MUST materializar a constante em AOT sem leitura de arquivo ou memória guest no runtime, sem interpretação/JIT e sem fallback silencioso. Falha de discovery MUST ocorrer antes da publicação do pacote.
- TLDR-08: o manifesto MUST continuar vinculado ao SHA-256 da imagem de entrada. A nova operação não exige mudança incompatível da representação pública; `GBA_IR_VERSION` permanece 1 enquanto não houver alteração de formato/semântica incompatível.

## Interfaces, entradas e saídas

O decoder produz uma operação Thumb pendente com `Rd` e endereço guest calculado. Discovery substitui essa forma por operação resolvida com `Rd`, endereço guest e `u32` lido de ROM. LLVM recebe somente o programa verificado e emite a constante no estado guest.

## Erros, limites e observabilidade

Uma carga parcialmente fora da imagem (mesmo que os primeiros bytes caibam), abaixo de `ROM_BASE`, desalinhada ou com overflow deve falhar em strict-AOT antes de existir diretório de pacote. Uma palavra dentro da ROM cujo valor seja um ponteiro fora dela pode ser lida normalmente; se fluxo posterior tentar `BX` para esse valor, a validação de alvo do branch deve rejeitá-lo separadamente. Nenhum desses casos habilita fallback ou mirror.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| Decode de `0x4A2C` em `0x080003B4` | `Rd=r2`, `imm8=0x2C`, endereço `0x08000468` |
| PC com `guest_pc % 4 == 2`, `imm8=1` | Para PC `0x08000086`, `Align(PC+4,4)=0x08000088` e alvo `0x0800008C` |
| Leitura de word ROM | Valor little-endian exato em r0–r7; flags preservadas, PC+2, ciclo+1 |
| `LDR literal` seguido de `BX` | Valor constante fecha só o alvo de controle alcançado pelo BX |
| Última word completa da ROM | Aceita se os quatro bytes cabem |
| Leitura que excede a ROM por um ou mais bytes | Discovery falha com PC/endereço e sem pacote parcial |
| IR pendente ou campos adulterados | Verificador rejeita antes do lowering |
| Outro load Thumb | Rejeitado em seu PC sem pacote parcial |
| O0 e O2 | Mesmo estado guest observável e manifesto strict-AOT com SHA da ROM |

## Questões abertas

Nenhuma para este subset. Mirrors e limites de cartuchos especiais precisam de contrato próprio antes de suporte.

## Referências e evidências

- [KNO-004](../knowledge/KNO-004-rom-intake.md): ROM e tentativa estática que alcançou `0x4A2C` após SPEC-024.
- [KNO-001 / SRC-26](../knowledge/KNO-001-evidence.md): observações de decode e execução nas referências fixadas.
- [SPEC-014](SPEC-014-rom-literal-loads.md): load literal ARM, contrato distinto.
- [SPEC-024](SPEC-024-thumb-lsl-immediate.md): incremento Thumb anterior.
