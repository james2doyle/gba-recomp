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

## Reprodução

Ler bytes localmente, calcular `hashlib.sha256`, extrair offsets 0xA0–0xBD e procurar padrões de SDK `FLASH1M_V[0-9]{3}` sem publicar payloads. Repetir abertura REA somente após suporte ao formato ou artefato derivado com origem explícita; renomear `.gba` para `.elf` não resolve o formato.
