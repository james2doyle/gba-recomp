# ADR-004 — Baseline de toolchain LLVM e Rust

- Estado: Accepted para o bootstrap GBA-010
- Data: 2026-10-13

## Contexto

O GBA-010 depende de versões reprodutíveis para a ferramenta Rust e para a API LLVM. A ABI do runtime precisa continuar utilizável sem LLVM; LLVM é dependência do processo de compilação AOT. O baseline inicial precisa produzir objetos ELF para Linux x86-64 e Mach-O para macOS arm64.

## Decisão

- Fixar Rust stable 1.99.0, edição 2024. O manifesto oficial da distribuição é identificado pelo SHA-256 em [`toolchain.lock.toml`](../toolchain.lock.toml).
- Fixar LLVM 23.1.3, tag `llvmorg-23.1.3`, commit `0d261d1ca552c95a8f007e061c787ac7132fbcbc`. Os pacotes oficiais para os dois hosts e o arquivo fonte têm hashes no lock.
- Fixar `llvm-sys = 231.0.1` e o SHA-256 do pacote registrado no lock. Desabilitar o link automático de bibliotecas e a inicialização de todos os targets em `llvm-sys`; `gba-llvm` valida `llvm-config` exatamente em 23.1.3 e linka apenas os componentes do lock. A crate fica encapsulada no backend `gba-llvm`; código unsafe da API C não atravessa os outros crates.
- Linkar estaticamente apenas os componentes LLVM declarados no lock. Usar o `clang` e `lld` da mesma distribuição LLVM para linkar o compilador host, além do sysroot do host. Os archives oficiais contêm bitcode LLVM 23; o Apple linker disponível usa LLVM 17 e não consegue lê-lo. macOS arm64 usa deployment mínimo 14.0 e link estático de libc++, libc++abi e libunwind do pacote LLVM para evitar substituir os runtimes do sistema. O objeto guest e o executável final do jogo continuam sem link ou dependência de LLVM.
- Exigir que `llvm-config --version` reporte exatamente `23.1.3`. Ausência, divergência de versão ou backend de destino ausente deve interromper a compilação com diagnóstico; não selecionar outra instalação nem fallback.
- Fixar os perfis de emissão nativa por host no lock: `x86_64-unknown-linux-gnu` com backend X86/CPU `x86-64`, e `aarch64-apple-darwin` com backend AArch64/CPU `generic`. Ambos usam conjunto explícito de features vazio, PIC e modelo de código `small`.
- Manter LLVM somente no processo de build. O objeto guest chama a ABI C versionada do runtime; o executável de jogo não inclui nem exige LLVM.
- O LLVM interno usado para compilar `rustc` não substitui a versão independente usada pelo recompilador.
- SDL3 continua candidato, sem versão fixada, até GBA-070, quando a escolha do adaptador desktop e suas necessidades serão avaliadas.

## Alternativas

`llvm-sys` não foi substituído por bindings próprios: a crate publicada corresponde à linha LLVM 23.1, cobre a API C proposta e evita manter declarações FFI locais antes do slice inicial. Um wrapper seguro próprio limitará a superfície unsafe. A dependência é pequena em escopo, mas sua manutenção passiva e a estabilidade limitada da API C exigem pin exato e revisão explícita para upgrades.

O código não depende de LLVM no runtime. Vincular o compilador LLVM dentro de cada jogo aumentaria o pacote e violaria SPEC-010 PORT-03.

## Consequências e gates

Os hashes de toolchain congelam entradas, não comprovam emissão, link ou execução. Os pacotes LLVM fixados publicam bibliotecas estáticas, sem `libLLVM` compartilhada; o build deve falhar se `llvm-config --shared-mode` divergir de `static`. GBA-010 deve validar os dois perfis host com a fixture ALU, registrar linker/sysroot efetivos, inspecionar o formato do objeto e comparar O0/O2. Até esse aceite, Linux x86-64 e macOS arm64 continuam planejados, não plataformas suportadas.

Atualizações de Rust, LLVM, `llvm-sys`, targets, features ou perfil de link exigem revisão deste ADR, atualização do lock e repetição do diferencial da fixture. Dependências transitivas Rust serão fixadas no `Cargo.lock` criado no GBA-010.

## Referências

- [SPEC-004](../specs/SPEC-004-ir-llvm.md) — lowering, TargetMachine, ABI e cache.
- [SPEC-010](../specs/SPEC-010-platform-packaging.md) — targets e distribuição sem LLVM no executável final.
- [KNO-001](../knowledge/KNO-001-evidence.md) — fontes, datas, hashes e limite da evidência.
- [KNO-003](../knowledge/KNO-003-open-questions.md) — emissão/link por host ainda pendente.
