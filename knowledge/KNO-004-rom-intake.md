# KNO-004 — Intake da ROM adicionada pelo usuário

Data: 2026-10-13. Artefato local em `roms/`, diretório adicionado ao `.gitignore` a pedido do usuário. Nenhuma ROM ou snapshot foi copiado para documentação.

## Observação por leitura local Python

| Campo | Valor |
|---|---|
| SHA-256 de todo o arquivo | `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825` |
| Tamanho | 16.777.216 bytes |
| Título do header | `POKEMON LEAF` |
| Game code | `BPGE` |
| Maker/revisão | `01` / `1` |
| Byte fixo | `0x96` |
| Checksum armazenado/calculado | `0x80` / `0x80` |
| Instrução inicial, leitura little-endian | `0xEA00007F` |
| Assinatura de backup encontrada | `FLASH1M_V103` |

Identidade em [rom-identity.json](evidence/rom-identity.json). Checksum calculado como `(-sum(bytes[0xA0:0xBD]) - 0x19) & 255`; não é hash de autenticidade. A interpretação ARM da instrução inicial aponta para `0x08000204`; isso é inferência estática limitada, não CFG recuperado. A assinatura sugere Flash 128 KiB, sem comprovar protocolo, saves ou ausência de periféricos.

## REA MCP

Chamada `open_binary` com o path absoluto da ROM retornou `target_unavailable`, categoria `unavailable`, razão `unsupported binary format`, `retryable=false`. Não houve decompilação, sessão nativa aberta nem execução. A skill de recuperação foi lida; não há motivo para reinstalar servidor ou engine para este erro de formato.

Questão registrada:
`unk_6ed4d5b1e57239579ee9218986275269f540616e5858fb89be3b27cd8e568c28`.
Evidence da criação do registro:
`ev_234bb65d5b29c13e0b06e62dc4ecb7982bb0b0a2ca1de9e44eef100a5c43c8d3`.
Esses IDs documentam a questão, não validam a ROM em execução.

## Resultado e próximo contrato

Criado [perfil draft](../profiles/leafgreen-bpge-rev1.toml), com hash exato e seed de entrada ARM. BIOS HLE, backup e RTC permanecem não resolvidos. Não foram acrescentados jump tables, overlays ou hooks especulativos. GBA-090 qualifica cenários após os marcos básicos; ainda não se pode afirmar que esse título compila ou executa.

## Pré-análise local GBA-020

Em 2026-10-13, o comando `gba-recomp inspect-rom` repetiu localmente SHA-256, tamanho e campos de header acima. `gba-recomp compile` começou a descoberta a partir do entrypoint e parou em `0x08000208`, palavra `0xE129F000`, com `unsupported instruction family is not implemented`. A saída não contém pacote; não houve decompilação REA nem execução da ROM. Isso mede somente o limite do decoder do commit `0dfbfb593b3882c6412f9048db786f064d881483`; não classifica semanticamente a instrução nem resolve a cobertura de LeafGreen.

## Tentativa incremental após SPEC-013/014

Com o CLI recompilado em 2026-10-13, ABI bancado e literal PC-ROM habilitados, a compilação avançou até `0x08000220`, encoding `0xE28F0020`, e parou com `data-processing opcode or flags mode is not implemented`. Nenhum pacote foi criado e LeafGreen não foi executado. A leitura do encoding como `ADD r0, pc, #0x20` e do valor aritmético `0x08000248` é inferência ARM; não demonstra que o resultado seja código executável nem que tenha sido consumido pelo jogo. O comando usa a ROM identificada pelo SHA-256 acima. O REA continua indisponível para esse formato; esta tentativa não acrescenta Evidence REA.

Após a fixture de `ADD` PC-relativo passar em O0/O2, uma nova compilação local avançou até `0x08000224`, encoding `0xE5810000`, e parou com `unsupported_instruction`. A leitura como `STR r0,[r1]` é inferência dos campos ARM. O pacote não foi criado e o jogo não foi executado. Esse limite requer escrita guest pelo bus; endereço e destino efetivos em runtime ainda não foram observados. Não extrapolar a instrução para cobertura geral de memória.

## Tentativa após SPEC-016 — RAM física word store

Em 2026-10-13, depois de `scripts/verify-gba020.sh` passar as fixtures ABI v3, uma nova compilação local falhou antes de publicar pacote em `0x0800022C`, palavra `0xE1A0E00F`, com `instruction family is not implemented`. A interpretação `MOV LR,PC` é inferência pelos campos ARM. O PC+8 para operando ARM está registrado como comportamento da referência no SRC-17, mas não demonstra o uso do resultado por LeafGreen. A ROM não foi executada nem decompilada; o relatório mede apenas o limite da descoberta AOT local para o SHA-256 fixado acima.

## Tentativa após SPEC-017 — MOV entre registradores

Em 2026-10-13, as fixtures do MOV `LSL #0` passaram em O0/O2. A compilação estática local avançou até `0x08000230`, raw `0xE12FFF11`, e parou com `instruction family is not implemented`. A leitura como `BX r1` é inferência pelos campos ARM. Nenhum pacote foi criado, a ROM não foi executada e o destino de `r1` não foi observado por trace; esta evidência apenas fixa o próximo limite de decode para o SHA-256 registrado acima.

## Tentativa após SPEC-018 — branch exchange ARM/Thumb

No commit local `fffdc2099a67e2fde58ee98ba7557ecc82cae2cb`, as fixtures sintéticas de BX e despacho por modo passaram em O0/O2. Uma nova compilação estática da ROM com o mesmo SHA-256 acima avançou até `0x080003A4`, halfword `0xB5F0`, e falhou com `unsupported_thumb_instruction`. A classificação como `PUSH {r4-r7,lr}` é inferência do formato Thumb, corroborada como observação de fonte em SRC-20. Nenhum pacote foi criado; LeafGreen não foi executada. Este resultado aponta apenas para a próxima lacuna alcançável observada pelo descobridor atual; não é um trace de execução nem inventário completo do jogo.

## Tentativa após SPEC-019/020 — PUSH e MOV high Thumb

No commit local `1011efd7d5fd298e6f165989968949f0658e5e64`, as fixtures estritamente AOT de PUSH Thumb e MOV high passaram em O0/O2 em macOS arm64, junto da suíte anterior. Nova compilação estática da mesma ROM passou pelas instruções em `0x080003A4` (`0xB5F0`, inferida como `PUSH {r4-r7,lr}`) e `0x080003A6` (`0x4647`, inferida como `MOV r7,r8`), e parou em `0x080003AA`, halfword `0x20FF`, com `unsupported_thumb_instruction`. A leitura de `0x20FF` como `MOVS r0,#0xFF` é inferência pelo formato, corroborada pelas fontes SRC-22. Nenhum pacote LeafGreen foi criado e a ROM não foi executada. O limite descreve a próxima instrução alcançável pela descoberta estática, não um trace de runtime nem a cobertura completa do título.

## Tentativa após SPEC-021 — MOVS Thumb imediato

No commit `508c51c98e50b8812e9ef9bc18f61997621fb3cc`, as fixtures estritamente AOT de MOVS Thumb imediato passaram em O0/O2, preservando C/V e atualizando N/Z. A compilação estática da ROM avançou de `0x080003AA` (`0x20FF`, inferido como `MOVS r0,#0xFF`) até `0x080003AC`, onde parou em `0xF1E3` (`unsupported_thumb_instruction`). A leitura estática do halfword seguinte em `0x080003AE` encontrou `0xFC0E`. Pelos campos e com a regra corroborada por SRC-23, esses halfwords formam provavelmente um BL Thumb: o LR parcial calculado é `0x081E33B0` e o destino `0x081E3BCC`, dentro da imagem ROM identificada. Isso é inferência estática, não trace nem execução. Nenhum pacote foi criado.

## Tentativa após SPEC-022 — BL Thumb estático

No commit `8db71bb5f91a569015ec643130d62d5427de1e22`, as fixtures de BL Thumb com retorno por `BX LR` passaram em O0/O2, mantendo as duas meias-instruções na IR e no orçamento AOT. A recompilação estática de LeafGreen passou pelo par raw `0xF1E3/0xFC0E` em `0x080003AC/0x080003AE` e alcançou `0x081E3BCC`, onde parou em `0xDF01` com `unsupported_thumb_instruction`. O halfword seguinte em `0x081E3BCE` é `0x4770`. Classificar `0xDF01` como Thumb SWI #1 (RegisterRamReset) é inferência pelo formato, corroborada pelas observações de fonte SRC-24; não é trace do jogo. O código de referência mostra que essa HLE pode limpar EWRAM/IWRAM, memória gráfica e registradores de I/O conforme `r0`; esses efeitos ainda não existem no runtime local. Nenhum pacote LeafGreen foi criado e a ROM não foi executada.

## Reprodução

Ler bytes localmente, calcular `hashlib.sha256`, extrair offsets 0xA0–0xBD e procurar padrões de SDK `FLASH1M_V[0-9]{3}` sem publicar payloads. Repetir abertura REA somente após suporte ao formato ou artefato derivado com origem explícita; renomear `.gba` para `.elf` não resolve o formato.
