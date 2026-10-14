# SPEC-039 — Thumb STR word imediato Format 9 em AOT estrito

Estado do documento: Accepted. Implementação e evidência: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Após elevar o budget de discovery pela SPEC-038, a tentativa estática para a ROM LeafGreen de SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825` avançou até o halfword observado `0x6005` em `0x080006B4`. Os campos inferem Thumb Format 9 `STR r5,[r0,#0]`; a classe é inferência do encoding, não execução guest.

Adicionar STR word imediato Thumb T1 a strict-AOT para stores dinâmicos em EWRAM/IWRAM físicas, usando `gba_store32_v3` já definido pela SPEC-016. Endereços guest seguem como u32 e entram no helper para validação. Stores para outras regiões retornam fault explícito; não há fallback.

## Fora de escopo

- Thumb LDR word imediato, transfers por registrador, SP-relative e demais formatos.
- Efeitos de MMIO, DMA, mirrors, ROM, BIOS, VRAM/OAM, backup ou bus geral.
- Normalização de EA desalinhado: o helper da SPEC-016 falha explicitamente em endereços não alinhados.
- Timing físico, Data Abort arquitetural, intérprete/JIT e suporte declarado a LeafGreen.

## Termos e invariantes

- T1: encoding Thumb com `(encoding & 0xF800) == 0x6000`; `Rd=encoding&7`, `Rb=(encoding>>3)&7`, `imm5=(encoding>>6)&31`.
- `old_Rb` e `old_Rd` são snapshots dos registradores guest antes do store, inclusive quando `Rd==Rb`.
- `guest_ea = old_Rb.wrapping_add(4 * imm5)`. A word é little-endian e só é gravada pelo helper em EWRAM/IWRAM físicas alinhadas.
- STR não altera flags, registradores nem writeback. Sucesso avança PC para `PC+2` e soma um ciclo abstrato. Fault não commita PC, ciclo, registradores ou memória.
- Discovery não pode preservar uma word antiga da sombra de memória se o store pode sobrescrevê-la; endereço Unknown limpa a sombra conforme SPEC-034.

## Requisitos normativos

- TSW-01: decoder e verificador MUST aceitar T1, conferir `Rd`, `Rb` e `imm5` contra o encoding e rejeitar formatos adjacentes não cobertos. Ambos os registradores são r0–r7.
- TSW-02: `ThumbOperation::StoreWordImmediate { rd, rb, imm5 }` MUST calcular o EA em aritmética u32 modular, com escala de quatro bytes, e capturar os valores-fonte antes da chamada ao helper. Não há flags, writeback ou acesso a `r15`.
- TSW-03: o lowering MUST validar overflow de ciclo antes do efeito, chamar `gba_store32_v3` uma vez e só commitar `PC+2`/ciclo+1 após sucesso. Fault retorna saída ABI sem mutação guest parcial. Nenhum endereço guest pode ser convertido em ponteiro host.
- TSW-04: `gba_store32_v3` MUST continuar validando ABI, memória, alinhamento de quatro bytes e intervalo físico de EWRAM/IWRAM segundo SPEC-016. Uma região não suportada ou EA desalinhado MUST produzir fault explícito com PC/endereço/código; sem intérprete ou JIT.
- TSW-05: a descoberta MUST calcular `old_Rb + 4*imm5` por estado quando possível e invalidar fatos sobrepostos de 4 bytes na sombra SPEC-034. Se o conjunto de endereços é Unknown, vazio, grande demais, cruza/wrapa intervalo ou não cabe em RAM física suportada, MUST limpar a sombra conservadoramente.
- TSW-06: o manifesto permanece `execution_mode="strict-aot"`. ABI v3/v4 e GBA IR v2 permanecem inalterados; esta operação não adiciona símbolo nem campo ABI.
- TSW-07: qualquer instrução alcançável sem lowering MUST falhar antes de publicar pacote parcial. Em runtime, regiões fora do subset falham explicitamente; não podem ser ignoradas ou convertidas em sucesso.

## Interfaces, entradas e saídas

- IR interna: `ThumbOperation::StoreWordImmediate { rd, rb, imm5 }`.
- Helper reutilizado: `gba_store32_v3(GbaRuntimeV3*, GbaExitV3*, uint32_t guest_address, uint32_t value)`.
- ABI, layout do runtime, versão do pacote e formato externo do manifesto não mudam. O GBA IR segue versão 2, pois `ArmProgram` não é serializado; a versão ABI v3 já aceita o helper de word-store.

## Erros, limites e observabilidade

O lowering reporta `fault` no PC da instrução e usa o EA guest original como `fault_address`. O helper pode rejeitar desalinhamento ou qualquer região fora de EWRAM/IWRAM físicas. O orçamento de execução e o ciclo continuam obedecendo aos contratos existentes.

## Critérios de aceite e matriz de testes

| Caso | Resultado verificável |
|---|---|
| `0x6005` | Decode como `STR r5,[r0,#0]` |
| Format 9, `imm5=31` | EA usa offset 124; little-endian; bytes vizinhos intactos |
| EWRAM/IWRAM O0/O2 | Estado guest e memória iguais; CPSR preservado; PC/ciclo avançam apenas em sucesso |
| `Rd==Rb` | Store usa o valor anterior do base/source |
| EA desalinhado, ROM, MMIO, BIOS ou fora da RAM física | Fault com PC e EA; nenhum commit parcial |
| Wrap u32 no cálculo do EA | Cálculo modular guest; helper rejeita EA resultante fora do subset |
| Discovery com endereço singleton e memória-sombra conhecida | Palavra sobreposta invalidada; palavras independentes preservadas |
| Discovery com endereço Unknown | Sombra limpa conservadoramente |
| `0x6800` e famílias adjacentes | Continuam rejeitadas sem pacote parcial |
| Fixture LeafGreen | Tentativa de compile registra nova fronteira e nunca afirma execução/compatibilidade por ter emitido IR |

## Questões abertas

- Quais writes DMA/MMIO e leituras de controle são necessários para que os stores observados na LeafGreen possam executar sem fault?
- Qual a fronteira de timing/eventos exigida pelo primeiro cenário de gameplay?

## Referências e evidências

- [SPEC-016 — STR word e helper ABI v3](SPEC-016-ram-word-stores.md)
- [SPEC-034 — memória abstrata de discovery](SPEC-034-abstract-memory-discovery.md)
- [SPEC-038 — budget de loops finitos](SPEC-038-discovery-state-budget.md)
- [ADR-026 — reutilizar o helper word-store](../adr/ADR-026-thumb-str-word-immediate.md)
- [KNO-004 — ROM e tentativa LeafGreen](../knowledge/KNO-004-rom-intake.md)
