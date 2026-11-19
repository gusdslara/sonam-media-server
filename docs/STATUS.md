# Estado da versão 0.1

## Implementado

- Servidor independente com SQLite e arquivos locais; frontend estático embutido.
- Svelte 5 usando `$state`/`$derived`, layout desktop/celular, navegação TV por setas.
- Primeiro acesso, login, sessão lembrada, logout, contas e regras por gênero/coleção.
- Upload em streaming, allowlist de containers, reprodução de original com Range, retomada por perfil.
- Player de vídeo, visualizador de fotos e player de áudio com fila, anterior/próximo, aleatório, repetição, busca e volume.
- Playlists por tipo, álbuns, associação de arquivos, nome/gênero editáveis, exclusão explícita.
- Pesquisa por nome/gênero e ordenação por data de upload/nome.
- Miniaturas locais de fotos com cache e limites de decodificação.
- Pareamento PIN/QR com aprovação por perfil autenticado, IP e QR em Configurações.
- Integração FFmpeg opcional: MP4 fragmentado, teste de encoders em hardware, GPU habilitada por preferência e opção CPU.
- Projeto Tauri desktop com servidor integrado; ponto de entrada mobile.
- Cliente Android TV/Fire TV com seletor de IP, sessão persistida, WebView e launcher Leanback.
- Cliente Roku SceneGraph com IP, pareamento, biblioteca, foto e players de áudio/vídeo.
- Docker Compose e exemplo de unidade systemd.
- Unidade de rede WebDAV (`/dav`) com pastas Biblioteca (monitorada, entra na biblioteca) e Arquivos; assistente para Windows; credenciais do Sonam.
- Nome `.local` por mDNS configurável, lista de endereços com QR, link HTTPS opcional por túnel Cloudflare.
- Backup opcional no Google Drive (login por código, `drive.file`): sincronização manual retomável com conferência MD5, pastas espelhando a unidade de rede, liberar espaço local, página Google Drive com as mesmas telas e reprodução pelo servidor, baixar de volta ao servidor ou ao aparelho.
- Servidor Android (`clients/android`): servidor Rust via JNI, serviço em primeiro plano, wake/Wi-Fi/multicast locks, início no boot, interface no app, escolha de armazenamento.

## Ainda necessário para uma primeira versão pública

1. Testar Windows/Linux empacotados e TVs físicas: decodificação, formatos, D-pad, tela cheia, consumo e reconexão. APK debug e ZIP Roku não são publicação em loja.
2. Testar o servidor Android em aparelhos físicos (Android 7 a 16, fabricantes com economia de bateria agressiva), consumo em longos períodos e reconexão de Wi-Fi. Assinatura de release e publicação. iOS não pode hospedar servidor em segundo plano.
3. Transcodificação adaptativa HLS/DASH, busca durante conversão, legendas, faixas, remux sem recodificação, HDR e limites configuráveis de consumo. FFmpeg/GPU precisam de validação de execução em hardware real.
4. Recuperação/alteração de senha, revogação e identificação de dispositivos, cotas, backup/restauração guiados, espaço livre, lixeira e upload retomável.
5. Indexação EXIF e timeline por data de captura, localização, OCR e pesquisa semântica local; rostos/objetos com modelos opcionais e opt-in. A pesquisa atual é textual.
6. Metadados de duração/codecs, capas extraídas, descoberta de pastas existentes, séries/temporadas, ordenação manual de playlists, renomear/excluir coleções, compartilhamentos mais granulares e paginação/virtualização para bibliotecas muito grandes.
7. Validar a unidade WebDAV com o WebClient do Windows, Finder e GVFS em uso prolongado; endereço público fixo sem conta (hoje: túnel rápido muda a cada reinício). Autoatualização assinada, CI em todas as plataformas e auditoria de segurança antes de recomendar exposição externa.

## Verificação

Os comandos, resultados e eventuais limitações da execução local são registrados em `VALIDATION.md`. A compilação estática de um cliente TV não comprova funcionamento em todas as TVs. O navegador define sua própria política de autoplay e a disponibilidade de codecs.
