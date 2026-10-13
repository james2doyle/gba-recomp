# SPEC-004 — GBA IR, LLVM e ABI de blocos

Estado do documento: Review. Revisão GBA-005 em 2026-10-13; aceite da implementação depende dos gates listados abaixo. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Preservar CPU e efeitos de hardware através de otimizações LLVM e emitir objetos nativos. LLVM não é decoder GBA nem implementação da PPU/APU.

## Requisitos normativos

- IR-01: GBA IR MUST ter versão, tipos de largura explícita, blocos, parâmetros SSA, terminadores e operações para registradores/flags, load/store guest, controle, ticks/checkpoints e exceções. Verificador MUST rejeitar tipo incorreto, uso sem definição, CFG inválido e efeito sem ordenação.
- IR-02: efeitos de memória, eventos, IRQ e guard MUST formar sequência explícita; barreiras semânticas não podem desaparecer por considerar helper puro. Otimizações MAY eliminar flags mortas apenas quando nenhum checkpoint/exception/fallback observa o estado intermediário.
- IR-03: lowering MUST usar i32 para endereços guest e auxiliares i64 quando necessário. Aritmética modular não recebe `nsw`/`nuw` sem prova. Shifts fora da largura e divisões excepcionais de helpers BIOS MUST ser tratados antes da operação LLVM que produziria poison/UB. `freeze` não substitui semântica guest.
- IR-04: guest pointer MUST ser resolvido pela ABI ou por acesso rápido comprovado a região; nunca `inttoptr` livre de endereço guest. Alinhamento, `inbounds`, `noalias` e atributos de memória só podem ser usados quando verdadeiros para aquele acesso.
- IR-05: backend MUST selecionar TargetMachine, triple, CPU/features, DataLayout, formato de objeto e relocation model, verificar LLVM IR antes/depois dos passes e guardar versões/pipeline. Baseline debug O0 e release O2; fast-math desligado. LTO só após evidência diferencial.
- IR-06: ABI C v1 MUST usar larguras fixas, publicar e validar versão/layout do contexto antes de executar; tipos LLVM/SDL/Rust enum cru não atravessam a ABI. Handles de periféricos futuros podem ser opacos.
- IR-07: bloco MUST poder retornar `continue`, `event`, `halt`, `missing_code`, `invalidated`, `fault` ou `stop`, com PC/modo e estado committed. Dispatcher externo evita crescimento ilimitado da pilha host. Interrupção e retomada intra-bloco exigem entradas de continuação ou materialização equivalente.
- IR-08: cache MUST incluir hash ROM/perfil/overlays/BIOS, IR/ABI/compiler/LLVM, passes, target/features e flags de link pertinentes. Preferências de apresentação não invalidam código.
- TC-01: o bootstrap MUST usar as versões e checksums de [toolchain.lock.toml](../toolchain.lock.toml). LLVM diferente de 23.1.3, crate `llvm-sys` diferente de 231.0.1 ou manifesto Rust diferente do fixado MUST falhar antes de emitir objetos.
- TC-02: o backend MUST habilitar os backends LLVM X86 e AArch64 e selecionar os perfis por target do lock. Features adicionais não listadas são proibidas na baseline; `DataLayout` MUST vir da `TargetMachine` selecionada e ser armazenada no manifesto/cache.
- TC-03: perfil inicial MUST emitir ELF64 para `x86_64-unknown-linux-gnu` e Mach-O 64-bit para `aarch64-apple-darwin`, com PIC e modelo de código small. O linker e sysroot nativos efetivamente usados MUST ser registrados pelo relatório GBA-010.
- TC-04: falha de `llvm-config`, incompatibilidade de versão/ABI, backend ausente ou target indisponível MUST retornar diagnóstico; não há seleção silenciosa de LLVM do sistema, outro target ou interpretação/JIT.
- TC-05: LLVM e bindings são dependências do recompilador no build. O código guest usa apenas a ABI de runtime C fixada; pacote de jogo MUST executar sem carregar ou localizar LLVM.
- TC-06: o compilador `gba-recomp` MUST linkar estaticamente apenas os componentes LLVM versionados no lock e MUST falhar se `llvm-config --shared-mode` não reportar `static`. Objetos emitidos e o executável final do jogo MUST permanecer sem dependência de LLVM.
- TC-07: o executável host do compilador MUST usar apenas `link_components` do lock e usar o `clang`/`lld` da distribuição LLVM fixada e o sysroot do host. No macOS, libc++, libc++abi e libunwind do pacote MUST ser linkados estaticamente para não substituir os runtimes carregados pelo sistema. O build MUST registrar linker e sysroot efetivos; Linux e macOS não podem selecionar silenciosamente o linker do sistema quando ele não lê o bitcode LLVM fixado.
- ABI-01: a ABI C v1 MUST expor `GbaRuntimeV1 { uint32_t r[16]; uint32_t cpsr; uint32_t abi_version; uint64_t guest_cycle; }` e `GbaExitV1 { uint32_t reason; uint32_t next_pc; }`. `GbaRuntimeV1` tem size 80/alignment 8, `cpsr` offset 64, `abi_version` offset 68, `guest_cycle` offset 72; `GbaExitV1` tem size 8/alignment 4. Rust e C MUST verificar esses layouts em compile time.
- ABI-02: `r[15]` contém o PC guest como inteiro; nunca é ponteiro host. `abi_version` MUST ser igual à versão 1 antes do bloco executar. Status v1 define `continue=0`, `event=1`, `halt=2`, `missing_code=3`, `fault=4`, `invalidated=5` e `stop=6`. O código gerado MUST validar ABI e overflow de ciclo antes de commitar registradores, PC ou ciclo; retorno e `out.reason` MUST concordar.
- ABI-03/04/05: ABI v2 é aditivo e preserva integralmente v1. Estado bancado, layout e `gba_block_v2`/helper `gba_msr_cpsr_v2` são definidos em [SPEC-013](SPEC-013-banked-cpu-and-psr.md); objetos v1/v2 não podem ser misturados.
- SLICE-01: a fixture GBA-010 representa uma instrução ARM `ADD r0, r1, r2` sem update de flags, em PC ARM alinhado. Com `r1=0xFFFFFFFF`, `r2=2`, `pc=0x08000000`, `guest_cycle=41`, o resultado é `r0=1`, `pc=0x08000004`, `guest_cycle=42`, CPSR inalterado e saída `continue`.
- SLICE-02: se o ciclo inicial for `UINT64_MAX`, o bloco retorna `fault`, sem alterar registradores, CPSR, PC ou ciclo. O caso exercita o commit atômico do slice e não define precisão temporal além desta fixture.
- SLICE-03: objetos O0 e O2 MUST linkar com o harness C da fixture e produzir o mesmo estado observável nos casos de sucesso e overflow. O executável fixture MUST funcionar sem carregar LLVM; a ferramenta que emite o objeto continua dependente de LLVM.

## Baseline de toolchain, targets e fixture GBA-010

| Host inicial | Rust target | LLVM backend/CPU | Features adicionais | Objeto |
|---|---|---|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu` | X86 / `x86-64` | nenhuma | ELF64 |
| macOS arm64 | `aarch64-apple-darwin` | AArch64 / `generic` | nenhuma | Mach-O 64-bit |

Ambos usam PIC e code model `small`. A fonte de verdade para versões, commits e hashes é o lock. O registro desses valores não prova emissão nem link; essa prova continua no gate GBA-010.

ABI e fixture:

```c
#include <stdint.h>

typedef struct GbaRuntimeV1 {
    uint32_t r[16];
    uint32_t cpsr;
    uint32_t abi_version;
    uint64_t guest_cycle;
} GbaRuntimeV1;

typedef struct GbaExitV1 {
    uint32_t reason;
    uint32_t next_pc;
} GbaExitV1;

uint32_t gba_block_v1(GbaRuntimeV1* rt, GbaExitV1* out);
```

`gba_block_v1` valida `abi_version`, executa o único `ADD` da fixture, avança PC/ciclo, e retorna `continue` ou `fault` por ABI incompatível/overflow de ciclo sem commit parcial. `r[15]` é endereço guest inteiro. O layout é mínimo para o slice ALU; novos campos exigem nova versão ABI ou extensão comprovadamente compatível.

```c
uint32_t gba_read_v1(GbaRuntimeV1* rt, uint32_t addr,
                     uint32_t width, uint32_t access_kind, uint32_t* value);
uint32_t gba_write_v1(GbaRuntimeV1* rt, uint32_t addr,
                      uint32_t width, uint32_t access_kind, uint32_t value);
uint32_t gba_checkpoint_v1(GbaRuntimeV1* rt, uint64_t guest_cycle);
```

`uint32_t` de retorno é status ABI definido; widths permitidas são 8/16/32 bits. Helpers futuros atualizam tempo via bus e comunicam eventos; checkpoint não pode mover relógio para trás. `GbaRuntimeV1`/`GbaExitV1` ficam congelados por offsets e sizes por target; estas assinaturas não autorizam structs divergentes entre objetos.

## Saídas, erros e aceite

`.gbair`, `.ll`, `.bc`, objetos e mapa guest→bloco/shard. Erros `ir_invalid`, `llvm_verify_failed`, `target_unavailable`, `abi_mismatch`, `link_failed` preservam diagnóstico e não publicam pacote incompleto.

Matriz: fixture GBA-010 O0/O2 e overflow de ciclo; depois shifts/overflow ARM; MMIO entre ALU; IRQ no meio de bloco; cache alterando cada fator; emissão/link em hosts iniciais; bloco com milhões de transferências sem stack overflow. Sharding determinístico tem orçamento mensurável, sem prometer tamanho/tempo antes das medidas.

## Referências e questões

KNO-001 fontes LLVM/toolchain, ADR-001/004 e SPEC-005. Emissão e link com as versões fixadas são gates de GBA-010.
