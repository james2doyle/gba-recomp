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

O projeto inclui intake com SHA-256 e um fluxo estritamente AOT ROM→LLVM→executável host para a homebrew headless Tiny Counter. Em macOS arm64, o gate headless passou em O0/O2 para as instruções e fixtures cobertas. Isso inclui ARM/Thumb `BX`, `PUSH`, MOV high, `MOVS`, LSL Thumb imediato, `BL` com retorno `BX LR` e Thumb SWI #1 (`RegisterRamReset`) por HLE versionada ABI v4; o runtime também testa `STR`/`PUSH` em EWRAM/IWRAM físicas por ABI v3. O pacote Tiny Counter tem apenas executável e manifesto e não carrega LLVM. O gate Linux x86-64 segue pendente; bus geral, mirrors, MMIO, BIOS restante, vídeo, áudio e input ainda não estão modelados. A compilação estática de LeafGreen passou por `0x081E3BCC` (`0xDF01`, inferido como SWI #1), retornou por `BX LR`, atravessou `0x080003B2` (`0x04C9`, inferido como LSL Thumb imediato) e parou em `0x080003B4` (`0x4A2C`, inferido como LDR literal). Nenhum pacote LeafGreen foi criado e o jogo não foi executado. [KNO-004](knowledge/KNO-004-rom-intake.md) registra as tentativas e [KNO-005](knowledge/KNO-005-isa-coverage.md) compara as famílias cobertas nas referências com o subset local. [ROADMAP.md](ROADMAP.md) é a fonte de progresso.

O workspace usa até dez jobs do Cargo e dez threads do LLD no link dos pacotes nativos, aproveitando as 10 CPUs lógicas do Mac mini M4 para acelerar compilações locais.

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
