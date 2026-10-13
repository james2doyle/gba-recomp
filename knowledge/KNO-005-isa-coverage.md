# KNO-005 — Inventário de cobertura ARMv4T

Data: 2026-10-13. Tipo de evidência: leitura estática dos três repositórios fixados; sem execução de instruções, ROM comercial ou emulador nesta análise.

## Revisões e escopo

| Fonte | Revisão | Arquivos consultados |
|---|---|---|
| gba-recomp | `de4edf59b872d887046d6a3b005e2df551b6d44c` | `crates/armv4t/src/{arm.rs,thumb.rs,lib.rs}`, `crates/gba-core/src/exec.rs`, `crates/recomp-core/src/{analyze.rs,emit.rs}` |
| mGBA | `143adf5aa2a60c6036810b0cd72445e13d74b0e6` | `src/arm/{decoder-arm.c,decoder-thumb.c,isa-arm.c,isa-thumb.c,arm.c}` |
| gba-recomp local | commit `8db71bb5f91a569015ec643130d62d5427de1e22` e ancestrais citados | `crates/gba-recomp/src/{decoder.rs,discover.rs,rom.rs}`, `crates/gba-ir/src/lib.rs`, `crates/gba-llvm/src/lib.rs`, `crates/gba-runtime-abi/src/` |

O inventário separa quatro etapas que não devem ser confundidas: reconhecimento do encoding, representação na IR, semântica guest no runtime e lowering AOT executável. Ler uma operação no decoder de referência não demonstra que o nosso runtime/LLVM a suporte. A tabela descreve código observado; não mede a cobertura alcançável de um jogo.

## Famílias observadas nas referências

| Família | Decoder ARM/Thumb de gba-recomp | Execução/recompilação de gba-recomp | mGBA |
|---|---|---|---|
| ALU e shifter | ARM registra os 16 opcodes, imediato rotacionado e shifts imediato/registrador; Thumb traduz formatos ALU para `Op::Alu` | `gba-core/src/exec.rs` implementa `Op::Alu`; o emitter tem caminhos nativos e shells de execução, inclusive `interp_one` | `decoder-arm.c` e `decoder-thumb.c` decodificam ALU; `isa-arm.c` e `isa-thumb.c` executam as operações |
| Multiplicação e swap | ARM registra MUL/MLA, long multiply e SWP/SWPB | `Op::{Mul,MulLong,Swap}` tem casos em `gba-core/src/exec.rs`; emissão deve ser lida junto dos shells e fallback | ARM decoder registra MUL/MLA, long multiply e SWP; handlers em `isa-arm.c` |
| Loads/stores simples | ARM word/byte, halfword e signed; offset imediato/registrador e pré/pós-indexação; Thumb cobre formatos próprios | `Op::Mem` unifica os formatos; execução chama `Bus`; parte pode sair por shell | ARM e Thumb têm decoders separados por formato; semântica usa callbacks de memória |
| Transferências múltiplas | ARM LDM/STM e Thumb LDM/STM, PUSH/POP | `Op::BlockMem` e execução de listas de registradores | ARM LDM/STM e formatos Thumb equivalentes |
| Desvios/interworking | ARM B/BL/BX; Thumb branches condicionais/incondicionais, BX e BL em duas metades | Branches diretos, lookup dinâmico e troca de modo integram análise/emissão | Decoders e execução separados preservam branch, link e troca ARM/Thumb |
| PSR/modos | ARM MRS/MSR para CPSR/SPSR, register/immediate, além de campos de mask | `Op::{Mrs,MsrReg,MsrImm}` e estado bancado | Decoder ARM e `isa-arm.c` tratam MRS/MSR; `arm.c` mantém registradores bancados |
| SWI/indefinidas | ARM/Thumb SWI; encodings indefinidos explicitados | Execução integra dispatch HLE/trap | Decoders ARM/Thumb produzem SWI/trap e handlers correspondentes |

Os detalhes de suporte em `gba-recomp` são observações do código nesta revisão, não resultado de testes reproduzidos. O próprio projeto declara fallback de interpretação para código não coberto pelo caminho emitido; por isso sua cobertura não pode ser copiada como uma alegação de AOT estrito.

## Cobertura deste projeto no estado local observado

| Família | Decode → IR | Lowering AOT | Evidência existente e limite |
|---|---|---|---|
| Predicação ARM e avanço de PC | Condições ARM 0–14 em cada instrução aceita | Predicado condicional no dispatcher | Fixture Tiny Counter e fixtures de condição falsa |
| ALU imediata | `MOV`, `ADD`, `CMP`; caso específico `ADD Rd,PC,#imm` | Implementados para o subset; PC visível é `guest_pc+8` | Tiny Counter e fixture SPEC-015; outros opcodes e operand2 registrador rejeitados |
| MOV ARM com operando registrador | Referências decodificam MOV pela família ALU e shifter | Forma `LSL #0`, sem flags e sem destino PC, implementada em SPEC-017 | Fixtures O0/O2 para `MOV LR,PC` e condição falsa; demais shifts seguem rejeitados |
| Branch ARM direto | B/BL com alvo relativo assinado | Dispatcher estritamente AOT | Tiny Counter O0/O2; não cobre `BX`, Thumb ou destino dinâmico |
| Branch exchange ARM/Thumb | `BX Rm` ARM/Thumb; propagação limitada a constantes e conjuntos finitos para fechar destinos | Atualiza CPSR.T/PC e despacha por `(modo, PC)` em switches AOT separados | Fixture ARM→Thumb→ARM O0/O2; BX ARM condicional falso, alinhamento ARM e rejeição de BX fora da ROM; não fecha alvos derivados de memória mutável |
| PSR | `MSR CPSR` por registrador, subset de campos aceito pelo contrato bancado | Runtime ABI v2 com bancos limitados aos modos modelados | Fixture v2; não cobre MRS/SPSR nem toda combinação de campos |
| LDR literal ARM de ROM | Palavra, imediato, pre-index, sem writeback, base PC; endereço e valor resolvidos na ROM imutável | Constante materializada em AOT | SPEC-014 O0/O2; não usa bus e não cobre memória dinâmica |
| STR word ARM imediato | Apenas pre-indexed, sem writeback, base/dado r0–r14; `Rn ± imm12` | ABI v3 chama helper para EWRAM/IWRAM físicas; endereço/alinhamento/versões são validados e faults retornam PC/endereço/código | A fonte mGBA confirma que stores passam pelo mapa do GBA; este subset local não cobre seus efeitos gerais |
| Outros loads/stores | Rejeitados | Nenhum | Loads dinâmicos, byte/halfword, offsets por registrador, pós-index, writeback e LDM/STM continuam fora do subset |
| Multiplicação, block transfer, restante ALU ARM | Rejeitados | Nenhum | Sem fixtures locais |
| ISA Thumb | `BX Rm`, `PUSH {rlist}`, MOV high-register, MOVS imediato e BL em duas metades; SWI #1 é uma operação separada | Lowering AOT do subset decodificado; BL grava LR e retorna pelo fluxo guest | Fixtures O0/O2 de interworking, cópia de registradores, stack, flags e BL/`BX LR`; shifts e demais instruções Thumb alcançáveis seguem rejeitados |
| Thumb LSL imediato | Ainda não decodificado; `0x04C9` em `0x080003B2` é inferido como `LSLS r1,r1,#19` | Nenhum | Próximo limite estático após SPEC-023, no commit `aab31ae`; raw e hash da ROM em KNO-004; SRC-25 corrobora formato/semântica de referência; SPEC-024 define o incremento pendente |
| Thumb SWI #1 / BIOS | Decoder preserva imm8; descoberta aceita somente #1 e fecha o fallthrough; outros immediates são rejeitados | `RegisterRamReset` HLE v1 em ABI v4; limpa as regiões e latches contratados, preservando CPU e CPSR.T | Fixtures AOT O0/O2 e teste C de máscaras, memória, DISPSTAT, VCOUNT, SOUNDCNT_X, IF, ABI inválida e falha atômica; SWIs restantes, bus geral e periféricos continuam ausentes |

As fixtures O0/O2 provam somente suas instruções e estado inicial conhecidos. `cargo test --locked --workspace` passou com 25 testes em `gba-ir` e 28 em `gba-recomp`; `scripts/verify-gba020.sh` passou em Darwin/arm64, incluindo as fixtures anteriores, HLE SWI #1, estado ABI v4, C v4 HLE e rejeições strict-AOT sem pacote parcial. Após SPEC-023, a compilação estática da ROM fixada passou por `0x081E3BCC` (`0xDF01`, inferido como SWI #1), pelo retorno `BX LR` em `0x081E3BCE` e parou em `0x080003B2` (`0x04C9`, inferido como LSL imediato). Isso registra a fronteira do descobridor AOT; não é trace de execução, nem afirma cobertura do restante da biblioteca GBA ou que o jogo compile.

## Evidência das implementações locais — SPEC-018/019/020/021/022/023

O commit `fffdc2099a67e2fde58ee98ba7557ecc82cae2cb` adicionou decoder, IR, análise estática, lowering LLVM e despacho ARM/Thumb para BX. `1011efd7d5fd298e6f165989968949f0658e5e64` adicionou PUSH Thumb com helper atômica da ABI v3 e MOV high-register. O commit `508c51c98e50b8812e9ef9bc18f61997621fb3cc` implementou MOVS Thumb imediato; `8db71bb5f91a569015ec643130d62d5427de1e22` implementou BL Thumb de duas metades. O commit `aab31ae40f64aeeba4a437ea34f36b8b3f08d0f4` adicionou decoder/IR/lowering strict-AOT do SWI Thumb #1 e HLE RegisterRamReset v1 em ABI v4 com estado de máquina v1. `cargo test --locked --workspace` passou com 25 testes em `gba-ir` e 28 em `gba-recomp`; `scripts/verify-gba020.sh` passou completo em Darwin/arm64, incluindo fixtures O0/O2, teste C do HLE, manifesto e rejeições sem pacote parcial; `cargo fmt --all -- --check` e `git diff --check` passaram. Isso valida programas sintéticos e macOS arm64, não o título comercial nem outro host.

A [SPEC-022](../specs/SPEC-022-thumb-bl-pair.md) e a [SPEC-023](../specs/SPEC-023-thumb-swi-register-ram-reset.md) estão implementadas nos subsets descritos. A tentativa LeafGreen após SPEC-023 está registrada em KNO-004: ela passou estaticamente pelo SWI #1 e retorno `BX LR`, mas parou em `0x080003B2`, raw `0x04C9`, inferido como `LSLS r1,r1,#19`. A [SPEC-024](../specs/SPEC-024-thumb-lsl-immediate.md) define o próximo incremento; sem pacote ou execução LeafGreen.

## Método recomendado para expandir cobertura

1. Manter uma matriz versionada por família/encoding que mostre separadamente decode, IR, semântica runtime, LLVM AOT e fixtures executadas.
2. Usar os decoders do gba-recomp como checklist de famílias e o mGBA como segunda implementação de referência para revisar regras; não transplantar código nem herdar conclusões de compatibilidade dos vendors.
3. Para cada incremento, escrever o contrato antes do código: campos aceitos, casos ARMv4T indefinidos, efeitos em PC/flags/bancos/memória, rejeições e critérios O0/O2.
4. Criar fixtures pequenas para cada ramo semântico e suas bordas. Quando o caminho de execução do jogo for instrumentável, registrar encodings e PCs alcançados separadamente de inferências estáticas.
5. Tratar erro de compilação como sinal de cobertura faltante e coletar novas observações, não como especificação suficiente. Agrupar trabalho por famílias e dependências de runtime, em vez de adicionar decodificações isoladas sem semântica.
6. Medir progresso do jogo por relatório de cobertura e execução com estado observável. Não afirmar recompilação ponta a ponta até emissão, link, execução e adaptadores do cenário passarem.

## Licenças e limites

Nenhum código foi copiado. mGBA declara MPL-2.0; gba-recomp declara MIT/Apache-2.0, com ativos/dependências de licenças próprias. Este documento usa nomes de operações e referências de arquivos como observação de fonte; eventual reutilização futura exige verificar licença por arquivo e registrar proveniência conforme [AGENTS.md](../AGENTS.md) e KNO-001.

O mapa de famílias é um inventário para orientar o roadmap, não uma auditoria formal de completude dos decoders dos vendors. A seleção de um emulador como referência semântica não transforma seus resultados em evidência de hardware real. REA não foi chamado nesta análise de código-fonte; nenhuma ROM foi decompilada ou executada.
