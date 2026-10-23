# SPEC-069 — Localização do frontend e troca de idioma em tempo real

Estado do documento: Draft. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

O frontend (`gba-desktop`, `gba-settings`, `gba-party`) tinha textos fixos em português, muitos deles escritos sem acento porque o menu na janela desenhava com a fonte de depuração da SDL (`SDL_RenderDebugText`, 8×8, apenas ASCII): `"Configuracoes"`, `"Nao"`, `"Nitido"`. Esta spec define a localização do frontend com idioma padrão `pt-BR`, número aberto de idiomas e troca de idioma em tempo real, sem reiniciar, incluindo a forma de alcançar o seletor em Android e Web, onde as janelas externas não existem.

Escopo: textos de interface dos três binários, o seletor de idioma no menu na janela, na janela de configurações e no menu nativo, a detecção do idioma do sistema e a fonte usada para desenhar texto no menu.

Idiomas embarcados: `pt-BR` (padrão) e `en-US`. Nenhum outro texto de interface pode ficar literal no código: uma string fixa em português dentro de `app.rs`, `gba-settings` ou `gba-party` fica em português para quem escolheu inglês e nenhum teste a detecta, então toda string de interface nova entra nos dois catálogos.

## Fora de escopo

Textos do `gba-recomp` (sinopse de comandos, flags e caminhos, sem prosa), mensagens do núcleo guest, tradução de conteúdo de jogo, formatação de datas/números/plurais por locale, escrita da direita para a esquerda (não há idioma RTL entre os suportados) e tradução de roteiros CJK no menu na janela (ver L10N-69-05).

## Termos e invariantes

- **Tag**: identificador BCP-47 do idioma (`pt-BR`). É o valor gravado em `settings.toml` e o nome do arquivo de locale.
- **Registro**: `gba_i18n::Lang`, a enumeração fechada de idiomas com tradução embutida. É a única fonte de verdade sobre "quais idiomas existem".
- **Locale corrente**: estado global do processo, alterado por `gba_i18n::set_lang`.
- Invariante: o idioma é dado de host. Nenhum valor de tradução pode alterar a simulação (SET-63-02 de [SPEC-063](SPEC-063-recompiled-frontend-settings.md)).
- Invariante: `pt-BR` é o idioma padrão e o fallback de qualquer chave ausente.

## Requisitos normativos

- L10N-69-01: `gba-i18n` MUST ser um crate do workspace cujo `Lang` enumera os idiomas com tradução, com `tag`, `display_name`, `language` (ISO-639), `from_tag` e `all`, e cujo macro `tr!` traduz uma chave no idioma corrente, inclusive com interpolação `%{nome}`. Acrescentar um idioma MUST ser uma variante em `Lang` mais um arquivo `locales/<tag>.toml` com as mesmas chaves; nenhum outro ponto do código MUST precisar mudar para o idioma aparecer no seletor. O roteiro está em [docs/adding-a-language.md](../docs/adding-a-language.md).
- L10N-69-02: `settings.toml` MUST aceitar a chave de topo `language`, cuja ausência vale `pt-BR`; arquivos anteriores MUST continuar válidos **sem** mudança de `version`, e uma tag desconhecida MUST ser rejeitada com mensagem que liste as tags disponíveis.
- L10N-69-03: trocar o idioma MUST refletir imediatamente no menu na janela, no menu de contexto nativo, na janela de configurações e no party, sem reiniciar o processo; a escolha MUST ser persistida. A propagação MUST valer nos **dois sentidos** entre janelas abertas ao mesmo tempo: um `language` gravado por outro processo MUST ser adotado em até ~1 s nos dois lados — o jogo que está rodando relê `settings.toml` a cada 500 ms, e `gba-settings` passou a fazer o mesmo. Uma janela MUST NOT descartar edições não aplicadas ao adotar a mudança alheia, mas o idioma MUST seguir mesmo assim, porque não é uma edição feita naquela janela.
- L10N-69-04: a detecção do idioma do sistema MUST usar `SDL_GetPreferredLocales`, que consulta Android, Web, Apple e Linux, com `$LC_ALL`/`$LC_MESSAGES`/`$LANG` como fallback no desktop e no CLI antes de o vídeo iniciar. Escolha explícita do usuário MUST prevalecer sobre o idioma do sistema, inclusive quando o sistema mudar (`SDL_EVENT_LOCALE_CHANGED`). A mesma regra MUST valer nos três frontends, que compartilham `settings::initial_language`: `gba-settings` e `gba-party` abrem no idioma salvo, ou no do sistema quando não há escolha. Uma tag de região sem tradução própria (`en-GB`, `pt-PT`) MUST resolver para a tradução da mesma língua antes de cair no padrão.
- L10N-69-05: o texto do menu na janela MUST ser desenhado por um atlas de glifos gerado no build a partir de `assets/fonts/PixAntiqua.ttf`, sem biblioteca de fontes em tempo de execução. Um caractere sem glifo MUST desenhar `?`, e um teste MUST falhar se qualquer mensagem embarcada usar um caractere sem glifo.
- L10N-69-06: em telas de toque (Android, Web móvel), o menu na janela MUST ser alcançável por pressão longa na área livre da tela e, no Android, pelo botão voltar; o botão voltar MUST NOT ser capturável como binding de jogo.
- L10N-69-07: a pressão longa MUST NOT disparar sobre os controles na tela, para que segurar um direcional durante o jogo não abra o menu, e MUST disparar no máximo uma vez por pressão.

## Interfaces, entradas e saídas

- `gba_i18n::{Lang, set_lang, lang, translate, translate_in, from_preferences, detect_from, posix_locale_vars, all_messages, tr!}`.
- `settings::{initial_language, button_label, BUTTON_LABEL_KEYS}`: a regra de idioma inicial compartilhada pelos três binários e os nomes de botão traduzidos.
- `sdl::preferred_locales() -> Vec<String>`: tags BCP-47 na ordem de preferência do sistema.
- `font::{SIZE, LINE_HEIGHT, MISSING, glyph(char), covers(char), width(&str), coverage(), ATLAS_WIDTH, ATLAS_HEIGHT}`.
- `touch::{LongPress, on_control, LONG_PRESS, LONG_PRESS_SLOP}`.
- `Painter::{text, text_width, widest}` e `Video::render_overlay` (leitura do backbuffer para verificação).
- Entrada: `settings.toml`, variáveis de ambiente de locale, eventos `LOCALE_CHANGED`, `FINGER_*` e `AC_BACK`.

## Erros, limites e observabilidade

- Chave ausente devolve a própria chave (o `tr!` do `rust-i18n` já faz isso); o teste de corpus impede que isso chegue ao usuário.
- `cmap`/`loca`/`glyf` malformados ou uma fonte que não seja bitmap falham o **build** com mensagem explícita, em vez de desenhar errado.
- `settings.toml` inválido continua preservado e o jogo usa padrões (UI-07 de SPEC-063), agora incluindo o idioma.
- Sem tradução para o idioma do sistema, o resultado é `pt-BR`, o que é observável pela comparação no teste de CLI.

## Critérios de aceite e matriz de testes

| Requisito | Verificação |
|---|---|
| L10N-69-01 | Testes de `Lang` (tag ida e volta, formas POSIX, rejeição de tag inválida), de interpolação e de uso de `tr!` **de outro crate**; `every_locale_declares_the_same_keys` compara os conjuntos de chaves e `locales_are_actually_translated` impede um catálogo copiado sem traduzir |
| L10N-69-02 | Round-trip de `settings.toml`, ausência da chave mantendo `version = 1`, tag desconhecida rejeitada listando as disponíveis |
| L10N-69-03 | Testes do menu (rótulos traduzidos e acentuados, título por página, seletor aplicado e devolvendo `Apply`), `switching_to_english_retranslates_every_row` (todas as linhas, inclusive os nomes de botão interpolados), teste do mesmo seletor no menu nativo e em `gba-settings`; `a_language_written_by_the_settings_window_is_adopted` (jogo adota) e `polling_adopts_an_external_change` / `external_change_keeps_unsaved_edits` (janela adota, sem perder edição pendente). Verificado também com processos reais: log `language changed to en-US` após reescrever o arquivo com o jogo rodando |
| L10N-69-04 | Teste contra o sistema real comparando `SDL_GetPreferredLocales` com as variáveis POSIX; teste de `detect_from` (`pt_BR`, `pt-BR`, `en_US`, `en-US`, lista ordenada); `explicit_language_beats_the_system`; testes de integração do CLI com `LANG` controlado (`pt_BR`, `en_US`, `ja_JP` sem tradução, `en_GB`/`pt_PT` por família de língua) |
| L10N-69-05 | Testes do atlas (cobertura consistente, glifos dentro dos limites, tabela ordenada, ponto sólido, contadores ocos, acentos acima da linha de base) e do corpus (todo caractere embarcado, **em todos os idiomas**, desenhável) |
| L10N-69-06 | Testes de `AC_BACK` abrindo o menu e navegando para trás, e de `captures_key` impedindo o binding |
| L10N-69-07 | Testes de `LongPress` com tempo injetado: dispara em 500 ms e não em 499 ms, uma vez só, cancelado por soltura e por arrasto, nunca sobre controle |

Aceitação exige ainda a verificação manual descrita em
[`docs/manual-test-i18n.md`](../docs/manual-test-i18n.md); nenhum item daquele roteiro foi
executado neste ambiente.

## Questões abertas

- **Verificação manual pendente**: janela real com o menu desenhado pela PixAntiqua, pressão longa em aparelho Android físico, botão voltar em Android, e o menu em pilha WebAssembly/Web. Nada disso é coberto pelos testes automatizados deste ambiente, que não tem dispositivo, emulador nem `emcc`.
- Cobertura de escrita: a PixAntiqua é Latin-1 mais poucas letras Extended-A. Um idioma cirílico, grego ou CJK exigiria uma segunda fonte e uma decisão sobre o atlas; o teste de corpus transforma isso em falha explícita, não em `?` silencioso.
- Plural e gênero: `tr!` não tem regras gramaticais. Suficiente para este conjunto de rótulos; revisitar se um idioma exigir concordância.
- Posição do seletor: hoje a última linha do menu principal. Se o menu crescer, revisar a ordenação.

## Referências e evidências

[SPEC-063](SPEC-063-recompiled-frontend-settings.md), [SPEC-064](SPEC-064-settings-app-input-devices.md), [SPEC-068](SPEC-068-multiplatform-targets.md), [SPEC-009](SPEC-009-settings-menu.md), [ADR-042](../adr/ADR-042-i18n-and-bitmap-font.md).

Evidência de execução (idiomas embarcados `pt-BR` + `en-US`): `cargo test -p gba-i18n -p gba-desktop -p gba-settings -p gba-party` → 62 testes de lib em `gba-desktop`, 12 + 3 cross-crate + 1 doctest em `gba-i18n`, 5 de CLI, 1 em `gba-settings`, 2 ignorados de propósito (geram PNG). Render real do menu nos dois idiomas via `Video::render_overlay` → PNG inspecionado, com as linhas de Controles traduzidas (`Direita teclado` / `Right keyboard`) e a linha mais larga em 109 px de 232 disponíveis. Smoke anterior, ainda válido: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy gba-desktop --rom games/rkanoid-v3.00.gba --frames 180 --no-splash` → `frames=180 presented=180 audio=on underruns=0 overruns=0`. `cargo check -p gba-i18n --target wasm32-unknown-emscripten` e `cargo build --workspace` exigem o LLVM estático provisionado; ver [KNO-011](../knowledge/KNO-011-i18n.md).

O que **não** foi executado: janela real, aparelho Android, iOS e build Web. Ver as questões abertas acima e o roteiro de verificação manual.