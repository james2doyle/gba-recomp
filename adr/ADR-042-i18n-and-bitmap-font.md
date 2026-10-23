# ADR-042 — Localização com `rust-i18n` e atlas de fonte gerado no build

- Estado: Proposed
- Data: 2026-10-23
- Supersede: —

## Contexto

O frontend tinha textos fixos em português, e o menu na janela desenhava com `SDL_RenderDebugText`, a fonte de depuração da SDL: 8×8, apenas ASCII. Por isso os rótulos estavam escritos sem acento (`"Configuracoes"`, `"Nao"`, `"Nitido"`) — não era estilo, era a única forma de aparecer algo legível. A [SPEC-069](../specs/SPEC-069-localization.md) pede idioma padrão `pt-BR`, número aberto de idiomas e troca em tempo real, e o produto precisa funcionar em macOS, Linux, Android, iOS e Web ([SPEC-068](../specs/SPEC-068-multiplatform-targets.md)).

Duas decisões independentes precisavam ser tomadas: como armazenar e trocar traduções, e como desenhar texto Unicode nos 240×160 do menu sem introduzir uma dependência nativa em cinco empacotamentos.

## Decisão proposta

1. **`rust-i18n` (`=4.2.4`)** como motor, num crate próprio `gba-i18n` do workspace. Arquivos `locales/<tag>.toml` são compilados para dentro do binário; `t!`/`tr!` resolvem a chave no locale global; `set_locale` troca o idioma sem reiniciar. A enumeração `Lang` é o registro único de idiomas.
2. **Fonte PixAntiqua embutida, rasterizada no build.** A PixAntiqua (Gerhard Großmann, SIL OFL 1.1) é uma fonte de bitmap: medindo o arquivo, todos os contornos são retângulos alinhados a eixos, com coordenadas exatamente na grade de 12 px e nenhum ponto fora da curva. `build.rs` lê `cmap`/`loca`/`glyf`/`hmtx` e gera um atlas de cobertura 1-bit em `OUT_DIR`, desenhado como uma textura branca tingida por `SDL_SetTextureColorMod`.
3. **Detecção pelo SDL**, via `SDL_GetPreferredLocales`, com variáveis POSIX como fallback apenas no desktop/CLI.

## Alternativas

- **`fluent`/`fluent-bundle`**: mais correto para plural, gênero e seleção, e é o que a Mozilla usa. Rejeitado nesta etapa porque o conjunto é de rótulos curtos sem concordância, e o custo é uma sintaxe e um modelo mental novos para toda a equipe contribuir.
- **`gettext`/PO**: bom para tradutores, mas exige catálogos binários fora do crate e um fluxo de extração que este projeto não tem.
- **Tabelas à mão em `gba-i18n`**: dispensaria dependência, mas obrigaria a reimplementar busca, fallback e interpolação, justamente onde erros silenciosos aparecem.
- **SDL_ttf + fonte TrueType embutida** (Noto Sans, e Noto CJK para asiáticas): cobre qualquer escrita com um rasterizador maduro. Rejeitado porque acrescenta uma **segunda biblioteca nativa** a cada um dos cinco empacotamentos (dylib no macOS, `.so` no Linux e Android, `.a` no iOS, port no Emscripten), incluindo CI e scripts de bundle — custo recorrente para um menu de 240×160. Com CJK seriam cerca de 20 MB por binário.
- **Manter `SDL_RenderDebugText` e só traduzir as janelas egui**: menor mudança, mas deixaria o menu na janela — onde o jogador realmente está — em português sem acento e sem seletor, e no Android e no Web as janelas externas não existem, então o idioma seria inalcançável.

## Consequências

- O idioma é dado de host e não toca a ABI guest; a separação de SET-63-02 permanece.
- A cobertura de escrita passa a ser uma propriedade **verificada**, não presumida: um teste percorre todas as mensagens embarcadas e falha se alguma usar caractere sem glifo. Isso já pegou um travessão (—) que desenharia `?`.
- Cyrillic, grego e CJK não são desenháveis no menu na janela. As janelas egui usam fontes do sistema e os mostram; o menu desenha `?`, e o teste falha antes de qualquer idioma assim entrar no registro.
- Avanços são inteiros por glifo, então o texto cai sempre em pixels inteiros e continua nítido em escala inteira. A largura medida difere marginalmente de um rasterizador que preserve avanços fracionários (81,949 px em "Configurações"), o que é o comportamento correto para uma fonte de bitmap.
- `gba-i18n` não depende de SDL: o frontend é que passa as tags. Isso evita ciclo de dependência e mantém o crate testável sem dispositivo.
- Acrescentar um idioma é uma variante em `Lang` e um arquivo `.toml`; o seletor e a persistência não mudam. Isso ficou demonstrado com `en-US`: nenhum código de menu, de janela ou de persistência foi tocado para o segundo idioma aparecer. O que **não** ficou coberto por essa regra foi o texto que nunca passou pelo catálogo — abas, botões e rótulos da janela de configurações, a visão de sala do party e as mensagens de save do jogo —, que estava literal em português e foi para os dois `.toml` neste passo. Uma string nova só continua traduzível se nascer como chave.
- O idioma corrente é guardado no próprio `gba-i18n`, não lido de `rust_i18n::locale()`, que começa em `"en"`. Sem isso, um processo que nunca escolheu idioma se declarava inglês enquanto ainda desenhava o fallback `pt-BR`. Efeito colateral bem-vindo: `lang()` não muda por um `set_locale` acidental, e `all_messages()` percorre os idiomas sem mexer no global, que é compartilhado entre threads de teste.
- Região sem tradução própria (`en-GB`, `pt-PT`, `fr-CA`) resolve primeiro pela variante exata e depois pela mesma língua, antes do padrão. Uma variante `en-US` não captura `en-GB`; a segunda passada só decide quando não há variante exata.

## Critérios para revisitar

- Se entrar um idioma com escrita não latina, ou RTL: revisitar o item 2 (segunda fonte no atlas, ou migrar para SDL_ttf e aceitar o custo nos empacotamentos).
- Se `en-US` ganhar uma variante regional de verdade (`en-GB` com ortografia própria), as duas precisam de arquivo e variante próprios; hoje `en-GB` cai em `en-US` por família de língua.
- Se aparecer necessidade de plural, gênero ou formatação de números/datas: revisitar o item 1 em favor de ICU4X ou Fluent.
- Se o número de chaves crescer a ponto de a extração manual virar erro recorrente: adotar `cargo i18n` (`rust-i18n-cli`) no CI.

## Referências

[ADR-001](ADR-001-core-architecture.md), [ADR-041](ADR-041-interpreter-first-core.md), [SPEC-063](../specs/SPEC-063-recompiled-frontend-settings.md), [SPEC-064](../specs/SPEC-064-settings-app-input-devices.md), [SPEC-068](../specs/SPEC-068-multiplatform-targets.md), [SPEC-069](../specs/SPEC-069-localization.md). Fonte: `assets/fonts/PixAntiqua.ttf`, licença SIL OFL 1.1 em `assets/fonts/PixAntiqua-LICENSE.txt`, extraída do próprio arquivo.
