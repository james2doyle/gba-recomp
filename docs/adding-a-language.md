# Como adicionar um idioma (SPEC-069)

Este roteiro é para quem vai acrescentar uma tradução. Ele não é a spec: os
contratos estão em [SPEC-069](../specs/SPEC-069-localization.md) (L10N-69-01) e a
justificativa da escolha em [ADR-042](../adr/ADR-042-i18n-and-bitmap-font.md).
Aqui ficam apenas os passos e as armadilhas que já custaram tempo.

## O que muda e o que não muda

Um idioma novo são **dois arquivos**: uma linha no registro `languages!` de
`Lang` (`crates/gba-i18n/src/lib.rs`) e `crates/gba-i18n/locales/<tag>.toml`.
Nada mais precisa ser tocado — nem o seletor do menu, nem `gba-settings`, nem
`gba-party`, nem a persistência. Se um desses pontos precisar de código para o
idioma aparecer, isso é um defeito de L10N-69-01, não parte do trabalho. A
linha é o único ponto: é ela que gera a variante, os dados do idioma e a
ligação com o arquivo (via `include_str!`), então um idioma sem `.toml` falha
no build em vez de aparecer vazio.

A tradução é embutida em tempo de compilação (`rust_i18n::i18n!` lê os `.toml` no
build), então **recompile** depois de editar um locale: o binário não lê o
arquivo em disco.

## 1. Registrar o idioma

Em `crates/gba-i18n/src/lib.rs`, acrescente **uma linha** no macro `languages!`:

```rust
languages! {
    default = PtBr;
    // ...idiomas existentes...

    /// Inglês do Reino Unido.          // doc da variante, se quiser
    EnGb: {                              // 1. nome da variante (ordem = seletor)
        tag: "en-GB",                    // 2. tag BCP-47 = nome do arquivo
        display: "English (UK)",         // 3. nome no próprio idioma
        language: "en",                  // 4. ISO-639, casamento por prefixo
        region: Some("GB"),              // 5. distingue variantes; None sem região
    }
}
```

- **`display` no próprio idioma** (`Deutsch`, não `Alemão`): quem está com a
  interface num idioma que não lê precisa reconhecer a própria língua na lista.
- **`tag` é o nome do arquivo** e o valor gravado em `settings.toml`.
- **A ordem das linhas** é a ordem do seletor no menu.
- `default = PtBr` não muda: o padrão e o fallback de chave ausente são
  `pt-BR` (L10N-69-02), e um idioma novo não altera isso.
- Idiomas sem variante de região traduzida (`pt-PT`, `en-GB`, `fr-CA`) já caem no
  idioma da mesma família por `from_tag`; só crie uma variante quando houver
  tradução distinta de fato.

## 2. Criar o arquivo de locale

Copie as chaves de `pt-BR.toml` — nunca de outro idioma já traduzido, que pode
estar desatualizado — e traduza os valores:

```sh
cp crates/gba-i18n/locales/pt-BR.toml crates/gba-i18n/locales/fr-FR.toml
```

### Restrições que o build/teste cobram

| Restrição | Por quê |
|---|---|
| Sintaxe TOML com `=`, uma chave por linha | `:` não é TOML; falha a compilação |
| Não declare `a.b` junto de `a` | colisão de prefixo do TOML: `menu.video = "..."` com `menu.video.scale` aborta no proc-macro |
| Sem `…`, `—`, `•`, aspas curvas | a PixAntiqua não tem esses glifos e o menu desenharia `?` |
| Sem emoji | idem |
| Mesmo conjunto de chaves que `pt-BR.toml` | chave faltando cairia no fallback e apareceria em português no meio da interface |
| Mesmos nomes de `%{placeholder}` que `pt-BR.toml` | `tr!` preenche por nome; um nome trocado sairia literal na tela |

Os dois primeiros são erros de build. Os dois últimos são testes que falham com
mensagem explícita. Para o problema de prefixo, a convenção adotada é separar o
nível semântico: `menu.video` (página) virou `menu.page.video`, e valores
auxiliares usam sufixo `_hint` (`settings.language_hint`).

### Interpolação

Valores com `%{nome}` recebem o argumento de mesmo nome do `tr!` no código:

```toml
menu.controls.keyboard = "%{button} clavier"
```

O nome precisa bater com o do chamador (`tr!("menu.controls.keyboard", button = label)`).
Um `%{botao}` por engano **não** quebra o build — sairia literal na tela —, por isso
`every_locale_uses_the_same_placeholders` compara os nomes de todos os idiomas com
os do `pt-BR` e falha apontando a chave. Se um idioma precisar de outra ordem de
palavras, basta reordenar os `%{...}` no valor: a ordem é do tradutor, não do código.

### Escrita não latina

Cyrillic, grego, árabe e CJK **não são desenháveis** no menu de 240×160 com a
fonte atual (L10N-69-05). O teste `every_shipped_locale_renders_in_the_menu_font`
falha antes de o idioma entrar. Nesse caso, pare e trate como decisão de
arquitetura (segunda fonte no atlas, ou migrar para SDL_ttf e aceitar a
dependência nativa nos cinco empacotamentos) — ver ADR-042, "Critérios para
revisitar". As janelas egui (`gba-settings`, `gba-party`) usam fontes do sistema
e não têm essa limitação; a restrição é só do menu na janela.

## 3. Verificar

```sh
cargo test -p gba-i18n -p gba-desktop -p gba-settings -p gba-party
```

| Teste | O que ele guarda |
|---|---|
| `every_locale_declares_the_same_keys` | você não esqueceu nem inventou chave |
| `every_key_exists_in_every_locale` | toda chave resolve para tradução (não devolve a chave crua) |
| `set_lang_round_trips_for_every_lang` | o idioma escolhido é o idioma relatado e usado |
| `every_locale_uses_the_same_placeholders` | nenhum `%{...}` foi trocado por engano |
| `locales_are_actually_translated` | o arquivo não foi copiado sem traduzir (valores idênticos são erro, exceto os cognatos declarados) |
| `tags_round_trip`, `accepts_posix_codesets_and_underscores` | `from_tag` reconhece a tag, inclusive `pt_BR.UTF-8` |
| `every_shipped_locale_renders_in_the_menu_font` | todo caractere embarcado tem glifo |

Depois, o roteiro em [manual-test-i18n.md](manual-test-i18n.md) — em especial a
troca em tempo real e a persistência, que os testes cobrem no modelo mas não
numa janela real.

## 4. Texto novo em código

Ao acrescentar uma string de interface, **não a escreva no código**: acrescente a
chave em **todos** os locales e use `tr!`. Uma string literal em português
dentro de `app.rs`, `gba-settings` ou `gba-party` fica em português para sempre,
inclusive para quem escolheu outro idioma — e nenhum teste a detecta. Só nomes
próprios, formatos (`3x`, `60 ms`), flags de CLI e mensagens de diagnóstico no
`stderr` continuam fora do catálogo.
