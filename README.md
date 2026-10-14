# GBA Recomp — recompilador experimental

Projeto de recompilador estático **GBA → GBA IR → LLVM IR → plataforma host**, com runtime de hardware, perfis de ajuste fino por jogo e menu de vídeo, áudio e controles.

```text
ROM .gba + perfil por SHA-256 + símbolos opcionais
  → intake → descoberta ARMv4T/Thumb → GBA IR verificada
  → LLVM IR → objeto para target triple → link com runtime e frontend
                                          ↓
                       executável nativo + assets + manifesto
```

O LLVM traduz a CPU; o runtime reproduz barramento, PPU, APU, DMA, timers, IRQ, BIOS e cartucho. “Qualquer plataforma” é uma arquitetura extensível: cada destino precisa de backend LLVM, toolchain de link, runtime e adaptadores host. Não significa suporte automático a todo SO.

O projeto inclui intake com SHA-256 e um fluxo estritamente AOT ROM→LLVM→executável host para a homebrew headless Tiny Counter. Em macOS arm64, o gate passa em O0/O2 para os subsets e fixtures cobertos, incluindo ALU ARM/Thumb, branches, PUSH/POP/LDMIA/STMIA, transfers físicos de EWRAM/IWRAM, `BL` e SWI #1 por HLE versionada. O pacote Tiny Counter contém executável e manifesto e não carrega LLVM. A SPEC-038 elevou para 128 os estados completos exatos por `(modo, PC)`, mantendo o teto global em 100.000. SPEC-039/040 adicionaram Thumb STR/LDR word imediato; SPEC-041/042 adicionaram LSR imediato e LDRH imediato; SPEC-044 adiciona apenas STRH para limpar o latch IME no subset strict-AOT. Stores e loads dinâmicos aceitam EWRAM/IWRAM físicas; o único MMIO de store coberto é IME com bit 0 limpo, e as demais regiões fora do contrato geram fault explícito. O gate Linux x86-64 continua pendente; bus geral, mirrors, leituras MMIO, IE/IRQ, BIOS restante, vídeo, áudio e input ainda não estão modelados. A tentativa estática mais recente da LeafGreen ainda termina em `BX r0` em `0x08000B6A`; não foi criado pacote nem houve execução do título ou decompilação pelo REA. A inspeção LLDB corrigida encontrou SP=`0x03007E14` e LR=`0x08000B95` em B48. No fallthrough, a STRH para IME em B58 não prova IF zero e invalida a sombra; o POP em B68 deixa r0 `Unknown`. O branch direto recupera LR e resolve esse BX. A origem da perda da prova de IF foi localizada em `0x080003B8`: um STRH para `0x05000000` limpa os fatos de I/O do analisador, embora esse endereço esteja fora da janela MMIO modelada. Isso é estado estático do compilador, não execução guest. [KNO-004](knowledge/KNO-004-rom-intake.md) registra a evidência e os limites; [KNO-005](knowledge/KNO-005-isa-coverage.md) separa decode, semântica runtime e lowering AOT.

O gate GBA-020 detecta o orçamento lógico do host (teto dez) e limita workers de fixtures e LLD por fase. No Mac mini M4, o padrão é Cargo `--jobs 10`, link do binário `gba-recomp` com LLD 10, grupos de fixtures até dez workers × LLD 1 e compilações individuais com LLD 10. Seis grupos independentes (LSL/LSR, ADD/SUB, Thumb Format 3, LDR word, LDRH e STRB/LDRB) rodam sequencialmente. Em três rodadas alternadas do lote STRB/LDRB, as medianas foram 5,51 s com 1 worker × 10 threads LLD e 2,11 s com 10 workers × 1 thread: 61,6% menos wall time nesse grupo sintético. O gate completo aquecido passou em 44,55 s; o run serial de equivalência também passou, sem comparar esses tempos como benchmark. Os logs, hashes e limites estão em [SPEC-047](specs/SPEC-047-logical-cpu-budget.md) e [sua evidência](knowledge/evidence/gba020-cpu-budget-accelerated-gate.json). Isso acelera a suíte de fixtures, não a compilação de uma ROM comercial: discovery e emissão LLVM de uma ROM continuam seriais, e LeafGreen ainda para na descoberta antes do link. É possível reduzir o teto com `GBA020_CPU_BUDGET=4`. Consulte também [ADR-021](adr/ADR-021-bounded-gba020-gate-parallelism.md) e a [evidência histórica SPEC-038–043](knowledge/evidence/gba020-thumb-strict-aot-parallelism-macos-arm64.json).

- [Arquitetura](docs/architecture.md) e [fluxo do usuário](docs/product-flow.md).
- [Especificações](specs/README.md), [decisões](adr/README.md) e [plano](plans/PLAN-001-delivery.md).
- [Baseline de toolchain](toolchain.lock.toml) e [decisão ADR-004](adr/ADR-004-toolchain-baseline.md).
- [Workspace Rust](Cargo.toml), [verificação GBA-010](scripts/verify-gba010.sh) e [verificação do primeiro homebrew](scripts/verify-gba020.sh).
- [Evidências](knowledge/KNO-001-evidence.md), [uso do REA](knowledge/KNO-002-rea.md) e [questões abertas](knowledge/KNO-003-open-questions.md).

## Validar o slice GBA-010

Instale as versões e dependências do host descritas em [toolchain.lock.toml](toolchain.lock.toml), defina `LLVM_SYS_231_PREFIX` para a raiz extraída de LLVM 23.1.3 e execute:

```sh
scripts/verify-gba010.sh
```

O script emite O0/O2, verifica o formato do objeto, linka e executa o harness C e grava o relatório sob `target/gba010/`. A validação nativa atual é macOS arm64; Linux x86-64 permanece pendente no roadmap.

## Compilar a fixture Tiny Counter

Com a mesma toolchain LLVM fixada, gere a ROM sintética, compile-a e execute o pacote nativo:

```sh
python3 fixtures/tiny-counter/make_rom.py --out target/tiny-counter.gba
cargo run -p gba-recomp -- compile --rom target/tiny-counter.gba \
  --out target/tiny-counter-o2 --opt O2 --steps 100
target/tiny-counter-o2/game
```

O resultado esperado é `r0=10`, PC `0x080000D0` e CPSR `0x6000001F`. O contador só exercita `MOV`, `ADD`, `CMP` e branches ARM. `gba-recomp inspect-rom --rom <path>` mostra a identidade validada; qualquer instrução alcançável fora desse subconjunto faz `compile` falhar antes de publicar um pacote.

Para repetir O0/O2 e as rejeições da fixture, execute `scripts/verify-gba020.sh`. O pacote contém `game` e `manifest.json`; não contém a ROM, objetos temporários nem dependências LLVM.
- [Perfil por jogo](specs/SPEC-008-game-profiles.md) e [menu de configuração](specs/SPEC-009-settings-menu.md).
- [Exemplos TOML](profiles/README.md) para contratos ainda não implementados.

Referências locais: `vendor-ref/gba-recomp` e `vendor-ref/mgba`. O mGBA foi clonado de https://github.com/mgba-emu/mgba.git; commits e licenças estão no registro de evidências. `vendor-ref/` permanece ignorado pelo Git. Uma ROM local foi identificada em [KNO-004](knowledge/KNO-004-rom-intake.md), com [perfil draft](profiles/leafgreen-bpge-rev1.toml). Compatibilidade comercial ainda não foi demonstrada. `roms/` também está ignorado pelo Git.
