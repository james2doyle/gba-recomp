# SPEC-013 — Bancos ARM e transferência de PSR

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo do incremento

Estender o estado CPU sem alterar o ABI v1 já usado pela fixture GBA-010. Este incremento habilita `MSR CPSR` ARM por registrador e trocas de modo para User/System, FIQ, IRQ, Supervisor, Abort e Undefined. Ele prepara o reset da ROM LeafGreen, mas não implementa Thumb, MRS, MSR para SPSR, exceções, bus ou runtime de hardware completo.

## Requisitos de estado e ABI

- BANK-01: ABI v2 MUST preservar o prefixo CPU do ABI v1 e acrescentar bancos explicitamente. `r[16]` sempre representa o banco ativo; r0–r7 são compartilhados; r8–r12 alternam entre banco comum e FIQ; r13–r14 usam bancos User/System compartilhado, FIQ, IRQ, Supervisor, Abort e Undefined.
- BANK-02: SPSR deve existir por modo de exceção: FIQ, IRQ, Supervisor, Abort e Undefined. User/System não têm SPSR. A indexação dos bancos deve ser nomeada no header público.
- ABI-03: `GbaRuntimeV2` MUST ter offsets `[r=0, cpsr=64, abi_version=68, guest_cycle=72, common_r8_r12=80, fiq_r8_r12=100, banked_r13_r14=120, spsr=168]`, size 192 e alignment 8 nos targets x86-64 Linux e arm64 macOS. `GbaExitV2` conserva size 8/alignment 4 e offsets 0/4. Rust, C e LLVM verificam o mesmo layout em compilação/emissão.
- ABI-04: funções compiladas para runtime v2 usam `gba_block_v2(GbaRuntimeV2*, GbaExitV2*)`; um objeto v1 e um objeto v2 não podem compartilhar a mesma entrypoint. O runtime C v2 é linkado estaticamente no pacote do jogo e não importa LLVM.
- ABI-05: `gba_msr_cpsr_v2` recebe apenas inteiros guest e o contexto ABI. Ele não recebe endereços guest como ponteiros; não expõe LLVM nem API host ao código guest.

## Semântica de `MSR CPSR, Rm`

- PSR-01: o decoder reconhece somente a codificação ARMv4T por registrador para CPSR. O subset inicial aceita máscaras `c` e `f`; campos `x`/`s`, destino SPSR e outras codificações permanecem explicitamente não suportados.
- PSR-02: predicação ARM é aplicada antes do efeito. Predicado falso avança PC e unidade de execução sem alterar flags, CPSR ou bancos.
- PSR-03: campo `f` escreve N/Z/C/V. Campo `c` escreve I/F e os quatro bits baixos de modo; bit T permanece no campo de estado e modo alto preservado. Escrita em modo User só pode alterar N/Z/C/V. Bits reservados permanecem preservados.
- PSR-04: troca de modo validada salva os registradores ativos no banco antigo e carrega o novo banco, sem tocar r0–r7 ou PC. A troca inclui a fronteira FIQ para r8–r12. O helper calcula e valida todo o destino antes de efetuar qualquer mutação.
- PSR-05: modo destino reservado/sem banco retorna `fault` sem mutação. Tentativa de alterar T retorna `missing_code` sem mutação enquanto Thumb não for compilado. ABI incompatível retorna `fault`. O dispatcher ARM retorna `missing_code` se CPSR.T já estiver ativo.
- PSR-06: instrução válida avança PC ARM em quatro bytes e soma uma unidade abstrata a `guest_cycle`; esta unidade não é timing ARM7TDMI e será substituída conforme SPEC-005.

## Critérios de aceite

| Caso | Resultado esperado |
|---|---|
| Layout Rust/C/LLVM v2 | Offsets, size e alignment iguais à tabela ABI-03 |
| System → Supervisor | SP/LR System salvos; SP/LR Supervisor carregados; modo/CPSR atualizado |
| Supervisor → FIQ → Supervisor | R8–R12 e SP/LR de cada banco preservados; retorno restaura valores anteriores |
| `MSR CPSR` em User | Atualiza só N/Z/C/V; bancos e campos privilegiados inalterados |
| Predicado falso | Nenhum efeito em CPSR/bancos; PC/ciclo avançam |
| Modo inválido ou mudança de T | Status documentado e estado inteiro byte-a-byte inalterado |
| Entrada `0xE129F000` | Decodificada como `MSR CPSR` com campos c/f e r0; execução depende de estado CPU iniciado corretamente |
| O0/O2 | Mesmo estado e status nos casos anteriores; pacote sem LLVM dinâmico |

## Limites e referências

Esta entrega não prova execução de LeafGreen. `MSR` para SPSR, instruções Thumb, exceções e instruções de memória seguem sem suporte. Fonte observada do mGBA está em KNO-001 SRC-11, commit fixado; ela documenta a referência local e não substitui a matriz independente de aceite.
