# PLAN-001 — Incrementos verticais

O status está exclusivamente no [roadmap](../ROADMAP.md). Cada incremento deve ter contrato aceito antes da implementação correspondente.

## 1. Fundação e CPU mínima — GBA-005/010/020

GBA-005 fixa Rust/LLVM/bindings e os perfis dos dois hosts em ADR-004 e `toolchain.lock.toml`. GBA-010 tem scaffold, ABI v1, IR mínima e emissão LLVM; a fixture O0/O2 passou em macOS arm64, e o gate Linux x86-64 permanece pendente. GBA-020 especifica em SPEC-012 e implementa intake SHA-256, descoberta de branches diretos, decoder ARM restrito, dispatcher LLVM por PC e pacote nativo headless. A ROM Tiny Counter percorre seis instruções AOT e O0/O2 produzem o mesmo estado após 100 passos em macOS arm64. O programa só exercita registradores e flags; memória, MMIO e periféricos são os próximos contratos. A execução Mac não declara o host Linux suportado nem fecha GBA-010.

## 2. Hardware e dispatch — GBA-025/030/040/045/050

GBA-025 implementou ABI v2, estado bancado e `MSR CPSR` da SPEC-013, literal PC-ROM restrito da SPEC-014, `ADD Rd,PC,#imm` da SPEC-015, `STR` físico limitado da SPEC-016, `MOV Rd,Rm` com `LSL #0` da SPEC-017, `BX` ARM/Thumb da SPEC-018, `PUSH` Thumb da SPEC-019, MOV high-register Thumb da SPEC-020, MOVS imediato da SPEC-021, BL Thumb de duas metades da SPEC-022, HLE Thumb SWI #1 RegisterRamReset da SPEC-023, LSL Thumb imediato da SPEC-024, LDR literal Thumb de ROM da SPEC-025, ADD/SUB Thumb Format 2 da SPEC-026, STRH Thumb imediato da SPEC-027, STRB Thumb imediato da SPEC-028, LDRB Thumb imediato da SPEC-029, ORRS Thumb Format 4 da SPEC-030, branch condicional Thumb da SPEC-032, POP baixo da SPEC-033, sombra limitada para descoberta da SPEC-034, LDMIA Thumb da SPEC-035, STMIA Thumb da SPEC-036 e especialização ROM multi-base/refinamento CMP-branch da SPEC-037. A SPEC-023 acrescenta ABI v4 e estado de máquina v1 só para os efeitos contratados; não fornece bus, PPU ou APU gerais. KNO-005 mantém decode, semântica runtime e lowering AOT em colunas separadas. As fixtures strict-AOT O0/O2 passaram para POP, LDMIA ROM singleton/multi-base e RAM dinâmica, STMIA física, retorno por LR salvo, ALU, loads/stores físicos, faults e rejeições sem pacote parcial; consulte os relatórios em `knowledge/evidence/`. A suíte completa e o gate foram executados em Darwin/arm64. A tentativa LeafGreen continua sendo apenas análise estática e parou pelo limite de 64 estados em `0x08000996`; não há pacote LeafGreen nem decompilação REA. O gate GBA-020 ocupa até oito workers × uma thread LLD nos grupos ADD/SUB independentes, mas a busca do CFG e emissão LLVM de uma ROM seguem seriais. Fragmentar a compilação de um jogo exige outra tranche e medição representativa.

SPEC-030 implementa ORRS Thumb Format 4 strict-AOT, SPEC-031 implementa MOVS/CMP/ADDS/SUBS Thumb Format 3, SPEC-032 implementa branch condicional Thumb Format 16, SPEC-033 implementa POP de r0–r7 via helper ABI aditivo e SPEC-034 acrescenta uma sombra esparsa, limitada e exclusiva da descoberta. O gate de SPEC-034 passou em O0/O2 com restauração do LR salvo e rejeição do BX quando LR não é conhecido; consulte a [evidência de memória abstrata](../knowledge/evidence/gba020-abstract-memory-macos-arm64.json). A descoberta estática de LeafGreen passou por `0x080009B6` (`0xBC08`, POP `{r3}` inferido), fechou o retorno em `0x080009BE` (`0x4700`, BX r0 inferido) e avançou a `0x080006A8`, raw `0xCB01`, inferido como Thumb LDMIA. Os valores de NZCV e a execução real do título continuam sem observação.

SPEC-035/ADR-022 implementam LDMIA Thumb strict-AOT. O helper aditivo é restrito a EWRAM/IWRAM; SPEC-037 especializa em constantes AOT LDMIA ROM com 2–16 bases provadas e testa falta de tabela como fault explícito. SPEC-036 implementa STMIA T1 strict-AOT para RAM física; as fixtures O0/O2 e os casos multi-base LDMIA estão cobertos nos relatórios em [evidence](../knowledge/evidence/). Na LeafGreen, a tentativa pós-SPEC-037 parou no orçamento de estados completos em `0x08000996`, sem comprovar passagem pelo LDMIA em `0x080006A8`, geração de pacote ou execução do título. A análise e geração LLVM de um único jogo ainda usam fluxo serial; o benchmark do gate mede as fixtures sintéticas.

## 3. Perfis e produto — GBA-060/070/080

Implementar parser v1 e validação de evidências, cache por digest, preferências separadas e menu aplicável em jogo. Validar perda de dispositivo, pause/resume, fullscreen rollback e herança global/jogo. Empacotar para Linux x86-64 e macOS arm64 e executar sem LLVM instalado. Gate: fluxo completo com controles, áudio, vídeo e save.

## 4. Primeiro título — GBA-090

Usar o hash de KNO-004, sem tomar nome do arquivo como identidade. O perfil existente é intake draft, não tuning confirmado. Descobrir entradas/overlays, SWIs e hardware efetivamente usado; registrar traces com REA quando o runtime existir. Fixar cenários de boot, navegação, entrada em gameplay e save/reload com input reproduzível. Medir misses e promover apenas os caminhos comprovados. Se código gerado exigir interpretação, declarar aot-compatible ou deixar strict-aot explicitamente bloqueado para aquele cenário.

## 5. Expansão — GBA-095/100

Repetir gates de emissão/link/runtime/frontend por destino; mobile e Web exigem seus adaptadores. Link/netplay e acessórios exigem especificações próprias, sem simular sucesso para hardware ausente.

## Riscos e mitigação

| Risco | Mitigação |
|---|---|
| LLVM otimiza semântica guest incorreta | IR de efeitos, verificador e diferencial O0/O2 |
| Timing aproximado causa regressão | Checkpoints e traces de bus/eventos antes de otimizar |
| Build de ROM cresce excessivamente | Shards, cache e budgets medidos |
| Cobertura incompleta parece sucesso | Rótulo por cenário e census de fallback |
| Fonte/oráculo repete mesmo bug | Fixture independente; hardware quando necessário |
| Licenças diferem entre módulos vendor | Inventário por arquivo antes de reutilização |

Fora da primeira entrega executável: compatibilidade universal, netplay, acessórios raros, ganhos de resolução interna do jogo e substituição de engines de áudio. Escopo pode crescer por novas specs, sem alterar retroativamente evidência existente.
