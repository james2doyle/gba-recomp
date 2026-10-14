# PLAN-001 — Incrementos verticais

O status está exclusivamente no [roadmap](../ROADMAP.md). Cada incremento deve ter contrato aceito antes da implementação correspondente.

## 1. Fundação e CPU mínima — GBA-005/010/020

GBA-005 fixa Rust/LLVM/bindings e os perfis dos dois hosts em ADR-004 e `toolchain.lock.toml`. GBA-010 tem scaffold, ABI v1, IR mínima e emissão LLVM; a fixture O0/O2 passou em macOS arm64, e o gate Linux x86-64 permanece pendente. GBA-020 especifica em SPEC-012 e implementa intake SHA-256, descoberta de branches diretos, decoder ARM restrito, dispatcher LLVM por PC e pacote nativo headless. A ROM Tiny Counter percorre seis instruções AOT e O0/O2 produzem o mesmo estado após 100 passos em macOS arm64. O programa só exercita registradores e flags; memória, MMIO e periféricos são os próximos contratos. A execução Mac não declara o host Linux suportado nem fecha GBA-010.

## 2. Hardware e dispatch — GBA-025/030/040/045/050

GBA-025 implementou ABI v2, estado bancado e `MSR CPSR` da SPEC-013, literal PC-ROM restrito da SPEC-014, `ADD Rd,PC,#imm` da SPEC-015, `STR` físico limitado da SPEC-016, `MOV Rd,Rm` com `LSL #0` da SPEC-017, `BX` ARM/Thumb da SPEC-018, `PUSH` Thumb da SPEC-019, MOV high-register Thumb da SPEC-020, MOVS imediato da SPEC-021, BL Thumb de duas metades da SPEC-022, HLE Thumb SWI #1 RegisterRamReset da SPEC-023, LSL Thumb imediato da SPEC-024, LDR literal Thumb de ROM da SPEC-025, ADD/SUB Thumb Format 2 da SPEC-026, STRH Thumb imediato da SPEC-027 e STRB Thumb imediato da SPEC-028. SPEC-029/ADR-019 aceitam LDRB Thumb imediato; implementação em andamento. A SPEC-023 acrescenta ABI v4 e estado de máquina v1 só para os efeitos contratados; não fornece bus, PPU ou APU gerais. KNO-005 mantém decode, semântica runtime e lowering AOT em colunas separadas. As fixtures strict-AOT O0/O2 passaram para ADD/SUB, propagação LDR→ADD→BX, limites ROM, stores halfword/byte, faults e rejeições sem pacote parcial; a evidência STRB e seus hashes estão em [gba020-thumb-strb-macos-arm64.json](../knowledge/evidence/gba020-thumb-strb-macos-arm64.json). `cargo test --locked --workspace --jobs 10` passou com 36 testes em `gba-ir` e 40 em `gba-recomp` (build de aproximadamente 75 s), e o gate completo passou em Darwin/arm64. A compilação estática de LeafGreen avançou de `0x080003B8`, raw `0x8008`, até STRB em `0x08000998`, raw `0x7005`, e parou em `0x0800099C`, raw `0x7808`, inferido como `LDRB r0,[r1,#0]`. Não houve pacote LeafGreen, execução da ROM ou decompilação REA. As SPEC-027/028/029 limitam stores halfword/byte e load byte às EWRAM/IWRAM físicas, sem criar bus geral. Depois, ampliar as instruções alcançadas e completar checkpoints, IRQ e DMA antes de otimizações agressivas. Fixture futura escreve tiles/bitmap, responde a botões, produz tom com timer/FIFO e persiste backup; introduzir código ROM→RAM e overlay para testar invalidação. mGBA fixado por commit fornece referência externa; discrepâncias viram KNO. Gate: mesmo cenário AOT gera estado e saídas esperadas.

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
