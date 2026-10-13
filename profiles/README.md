# Perfis e exemplos

Contrato normativo: [SPEC-008](../specs/SPEC-008-game-profiles.md). Não há parser implementado; os exemplos são TOML válido e servem de entrada para sua futura implementação.

- [Perfil da ROM local](leafgreen-bpge-rev1.toml): SHA-256 real, status draft, somente entrada inicial e hipótese de backup. Não atesta compatibilidade ou ajuste de performance.
- [Preferências globais](examples/settings.toml): exemplos de vídeo, áudio e input conforme SPEC-009.
- [Override do jogo](examples/game-settings.toml): somente preferências explicitamente substituídas; demais campos herdam valor global.

Snapshots RAM e ROMs ficam locais; perfis registram identidades e endereços. O build deve recusar campos técnicos ainda não resolvidos antes de publicar um pacote validado.
