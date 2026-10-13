# ADR-002 — AOT estrito, fallback explícito e perfis vinculados ao conteúdo

- Estado: Proposed
- Data: 2026-10-13

## Contexto

Descoberta estática não prova todos os saltos indiretos; código pode ser copiado, descomprimido ou modificado em RAM. O mesmo título pode ter revisões incompatíveis.

## Decisão proposta

Publicar modos `strict-aot` e `aot-compatible`, ambos sem JIT. O primeiro falha em misses; o segundo pode interpretar e mede o uso. Guardar traduções RAM por conteúdo, localização e estado ISA. Perfis técnicos usam SHA-256 exato e revisão de schema; preferências do usuário têm armazenamento separado e herança explícita.

## Alternativas e consequências

Fallback silencioso elevaria boot rate, mas esconderia cobertura AOT. Traduzir todo offset aumentaria tamanho e ainda não resolveria código gerado. Perfis por game code facilitariam busca, mas não distinguiriam patches e revisões. Guards e invalidation custam tempo; removê-los exige prova por região e deve falhar quando a precondição deixar de valer.

## Revisão

Revisitar política de guards quando traces de CPU, DMA e overlays provarem uma alternativa equivalente. A evidência de uma sessão não prova cobertura universal.

Referências: [SPEC-003](../specs/SPEC-003-discovery-overlays.md), [SPEC-008](../specs/SPEC-008-game-profiles.md).
