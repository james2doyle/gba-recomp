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

O projeto inclui intake com SHA-256 e um fluxo estritamente AOT ROM→LLVM→executável host para a homebrew headless Tiny Counter. Em macOS arm64, o gate passou em O0/O2 para as instruções e fixtures cobertas, incluindo ALU ARM/Thumb, branches, PUSH/POP/LDMIA/STMIA Thumb, loads/stores físicos de EWRAM/IWRAM, `BL` e SWI #1 por HLE versionada. O pacote Tiny Counter tem apenas executável e manifesto e não carrega LLVM. O gate Linux x86-64 segue pendente; bus geral, mirrors, MMIO, BIOS restante, vídeo, áudio e input ainda não estão modelados. A SPEC-034 acrescentou uma sombra abstrata limitada a 64 palavras de pilha rastreadas por PUSH/POP; ela é somente de descoberta e não muda a ABI nem a memória guest. A SPEC-035 adicionou LDMIA Thumb strict-AOT, com loads ROM materializados quando a base é singleton e helper dinâmico para EWRAM/IWRAM. A SPEC-036 adicionou STMIA Thumb strict-AOT em EWRAM/IWRAM. A SPEC-037 acrescentou estados disjuntivos limitados, refinamento de `CMP`/branch imediato ARM e Thumb, e tabelas AOT para LDMIA ROM com 2–16 bases. As fixtures novas O0/O2 executam e despacham as words corretas. Na LeafGreen de SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825`, a tentativa estática atual parou pelo limite de 64 estados em `0x08000996`; não houve pacote ou execução LeafGreen. Isso não prova que a descoberta processou o novo LDMIA em `0x080006A8`. REA não produziu decompilação. [KNO-004](knowledge/KNO-004-rom-intake.md) registra a trilha e [KNO-005](knowledge/KNO-005-isa-coverage.md) separa decode, semântica runtime e lowering.

Cargo usa até dez jobs. A descoberta do CFG e a emissão LLVM de um único jogo ainda são seriais; o link de uma compilação individual aceita até dez threads LLD. No gate GBA-020, sete fixtures LSL e oito fixtures ADD/SUB independentes podem compilar em paralelo, cada link com uma thread LLD e o total limitado a dez slots configuráveis. O padrão atual usa oito workers × uma thread no grupo ADD/SUB. Três medições por configuração no grupo ADD/SUB deram medianas de 2,07 s com sete workers × uma thread e 4,23 s com um worker × dez threads; é throughput da suíte sintética, não da compilação de uma ROM comercial. Uma rodada aquecida do gate inteiro durou 48,85 s, sem uma comparação equivalente anterior. Veja [ADR-021](adr/ADR-021-bounded-gba020-gate-parallelism.md), [KNO-004](knowledge/KNO-004-rom-intake.md) e a [evidência SPEC-037](knowledge/evidence/gba020-thumb-ldmia-rom-cases-macos-arm64.json).

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
