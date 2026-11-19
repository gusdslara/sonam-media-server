# Sonam

Sua biblioteca de vídeos, músicas e fotos, na sua própria rede. Servidor Rust + SQLite, interface Svelte 5 com runes e aplicativo Tauri 2. Sem conta externa, telemetria, catálogo remoto ou assinatura.

**Versão 0.1: base funcional em desenvolvimento.** Há clientes TV iniciais, mas a validação de codecs e controles em TVs físicas ainda está pendente. Não considere esta versão um substituto completo do Jellyfin ou Google Fotos. Veja [STATUS.md](docs/STATUS.md) para os limites e a validação realizada.

O projeto se chamava HomeStream. As variáveis `HOMESTREAM_*` continuam aceitas quando a `SONAM_*` correspondente não existe, e o aplicativo desktop move sozinho a biblioteca da pasta antiga (`org.homestream.app`). Os apps Android têm novo identificador (`org.sonam.*`) e são instalados ao lado dos antigos.

## Começar

### Windows / Linux com código-fonte

Instale Node.js 24, Rust estável e ferramentas C/C++ para compilar SQLite. No Windows, use as ferramentas de compilação compatíveis com sua instalação do Rust. Para o aplicativo Tauri, siga também os [pré-requisitos oficiais](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run build
cargo run --release --locked --manifest-path server/Cargo.toml
```

Abra **http://127.0.0.1:8096** e crie o administrador. A senha precisa ter pelo menos 10 caracteres. No Windows, também é possível executar `powershell -File scripts/start.ps1`; no Linux, `sh scripts/start.sh`.

A conta de administrador é criada na primeira tela. No próprio servidor (o computador onde ele roda, ou o app no celular servidor) não se pede nada além de nome e senha. Por outro aparelho da rede, a tela explica e pede a **chave de primeiro acesso**, exibida no terminal/janela do servidor ou em `docker compose logs`. Isso impede que outro aparelho da rede tome um servidor recém-instalado.

Depois de compilar, o executável do servidor funciona sem Node ou Rust instalados. A interface já está embutida nele. O banco e os arquivos ficam em `data/` por padrão. Não coloque `data/` em um diretório servido por outro servidor HTTP.

### Linux com Docker

```sh
docker compose up -d --build
docker compose logs sonam
```

Os dados ficam no volume `library`. Use a chave exibida nos logs para o primeiro acesso remoto. A imagem inclui FFmpeg e executa como usuário sem privilégios. Para Intel/AMD, há instruções de `/dev/dri` no `compose.yaml`; NVIDIA requer o runtime de containers do fabricante e configuração própria. Sem acesso à GPU, continua funcionando por reprodução direta ou conversão por CPU.

O arquivo `deploy/sonam.service` é uma alternativa para um binário Linux instalado em `/usr/local/bin/sonam-server`, com usuário de serviço `sonam`. Ele não instala nem altera serviços automaticamente.

### Aplicativo desktop Tauri

```sh
npm run desktop:dev
npm run desktop:build
```

O aplicativo inicia o mesmo servidor na porta 8096 e abre a interface. Os dados ficam no diretório de dados do aplicativo do sistema, separado do `data/` da execução por terminal. Mantenha o aplicativo aberto para servir mídia. Não execute dois servidores na mesma porta. Os instaladores são gerados em `src-tauri/target/release/bundle/`; uma compilação com `--debug` vai para `target/debug/bundle/`.

## Usar em outras telas

1. No servidor, abra **Configurações**. O primeiro endereço é o nome **`http://sonam.local:8096`**, anunciado por mDNS: não muda quando o roteador troca o IP. Também aparecem os IPs e um QR para cada endereço.
2. Abra esse endereço no aparelho da mesma rede. Na TV com navegador, use `http://sonam.local:8096/?tv=1` (ou o IP, se a TV não resolver nomes `.local`).
3. Na nova tela, escolha **Entrar com um código (TV, sem digitar senha)**. Aparece um código de seis dígitos com QR.
4. Num celular ou computador que já entrou, leia o QR com a câmera ou abra **seu perfil › Parear tela** e digite o código. A nova tela entra sozinha, com o perfil de quem autorizou. O código é renovado automaticamente a cada cinco minutos.

O código expira em cinco minutos. A sessão permanece por até 30 dias, até sair ou expirar. A tela recebe as permissões do perfil que autorizou. Windows 10+, macOS, iOS e Linux com Avahi resolvem `.local`; alguns Android, TVs e o Roku não, então use o IP ou o QR neles. Reserva DHCP no roteador continua recomendada. O nome pode ser trocado em Configurações (por exemplo, `casa-da-vo.local`) quando houver mais de um servidor na rede.

### Nome curto e domínio próprio

- **`http://sonam`**: o Windows resolve o nome curto na rede de casa. Quando o servidor roda num computador e a porta 80 está livre, ele também escuta nela, então o endereço fica sem `:8096` (desative com `SONAM_PORT80=off`). No celular servidor o Android não permite a porta 80: use `http://sonam:8096`.
- **`http://sonam.local`**: Mac, iPhone, Linux com Avahi e a maioria dos Android.
- Os dois nomes só existem dentro de casa. **IP público:** crie um nome DNS de verdade (DuckDNS e No-IP são gratuitos) apontando para o roteador, encaminhe a porta e informe o endereço em **Configurações › Seu domínio**; o servidor passa a aceitá-lo e a mostrá-lo nos endereços e no assistente do Windows. Prefira HTTPS na frente (proxy reverso): sem ele, senha e arquivos trafegam sem cifra pela internet.
- **Tailscale** com o aparelho chamado `sonam`: `http://sonam:8096` funciona de qualquer lugar (MagicDNS), sem abrir portas.

### Fora de casa

- **Link com um clique (Cloudflare):** com o [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) instalado no servidor (ou `SONAM_CLOUDFLARED` apontando para ele), ative **Link para acessar fora de casa** em Configurações. O servidor cria um túnel rápido e mostra um endereço `https://….trycloudflare.com` com QR. Não precisa abrir portas no roteador e o tráfego é HTTPS. O endereço muda a cada reinício e túneis rápidos não têm garantia de disponibilidade. O túnel usa uma porta interna própria: quem chega por ele nunca é tratado como acesso local (a chave de primeiro acesso continua exigida) e o limite de tentativas é por visitante.
- **Tailscale:** instale nos seus aparelhos e no servidor. O endereço “Rede privada” (100.x.y.z) aparece em Configurações e funciona de qualquer lugar sem expor nada publicamente. É o caminho recomendado para o servidor no celular.
- **Domínio próprio:** túnel nomeado ou proxy reverso com TLS e `SONAM_PUBLIC_HOST`.

- **Android TV / Fire TV:** projeto em `clients/android-tv`, com launcher de TV, controle por D-pad, cookies persistidos e reprodução pelo WebView do aparelho. Leia [clients/README.md](clients/README.md) para compilar e instalar o APK.
- **Roku:** cliente próprio em BrightScript/SceneGraph em `clients/roku`, com entrada de IP, PIN, sessão lembrada, lista de mídia e players nativos. Não usa WebView/Tauri. `npm run roku:build` gera o ZIP para sideload em modo de desenvolvedor. Ainda requer validação em Roku físico.
- **iPhone / iPad (iOS 15+):** cliente nativo SwiftUI + WKWebView em `clients/ios`, com tela de endereço do servidor (IP da rede local ou `sonam.local`), sessão lembrada, upload de fotos e música com a tela bloqueada. É só cliente: não roda servidor no iPhone. O `.ipa` sem assinatura é gerado pelo GitHub Actions e instalado por sideload; veja [clients/README.md](clients/README.md#ios).
- **Celular como cliente:** o navegador funciona como cliente em qualquer celular.
- **Celular antigo como servidor (Android 7.0+):** o app em `clients/android` embute o mesmo servidor Rust e o mantém em um serviço em primeiro plano, com a interface dentro do próprio app (sem navegador separado). Veja [Servidor no Android](#servidor-no-android).

## Servidor no Android

Transforme um celular antigo em servidor da casa: fotos, músicas, vídeos e unidade de rede, servidos pelo Wi-Fi para TVs, computadores e outros celulares.

- **Segundo plano de verdade:** serviço em primeiro plano (notificação fixa com o endereço e botão Desligar), *wake lock* parcial para servir com a tela apagada, trava de Wi-Fi contra economia de energia e trava multicast para o nome `.local`. Continua rodando ao fechar o app; volta sozinho após reiniciar o celular ou atualizar o app (opção “Iniciar junto com o celular”).
- **Interface no próprio app:** o app abre a mesma interface Svelte em uma WebView ligada a `127.0.0.1`. Envio de fotos/vídeos pela galeria, tela cheia e botão Voltar funcionam. Não é preciso deixar um navegador aberto.
- **Armazenamento:** memória interna do app (padrão), cartão SD ou a pasta pública `Armazenamento interno/Sonam`, que continua após desinstalar (pede permissão de acesso a arquivos). Ao desinstalar, o Android pergunta se deve manter os dados do app.
- **Menu do app:** compartilhar endereço, funcionar com a tela apagada (isenção de otimização de bateria), local dos arquivos, dicas de uso e ligar/desligar.
- **Limites:** sem FFmpeg no celular, então não há modo compatível (reprodução direta funciona). Sem link Cloudflare no celular; use Tailscale. Fabricantes como Xiaomi, Samsung e Huawei têm economia de bateria própria: marque o app como “sem restrições”. Deixe o aparelho na tomada, de preferência com limite de carga ativado.

Compilar (requer Rust, Android NDK, SDK 36 e JDK 17/21):

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
npm ci && npm run build
python scripts/android-rust.py            # arm64-v8a + armeabi-v7a (celulares 32-bit antigos)
cd clients/android && ./gradlew assembleDebug
```

Use `--abi x86_64` para o emulador. O APK fica em `clients/android/app/build/outputs/apk/debug/`. `./gradlew -PbuildRust assembleDebug` executa o script do Rust antes. Instale apenas em aparelhos seus, com depuração autorizada (`adb install`).

## Unidade de rede

Em **Conta › Unidade de rede** (ou Configurações), o Sonam vira um disco no computador, como uma pasta compartilhada:

| Sistema | Como conectar |
|---|---|
| Windows | Baixe o assistente `conectar-sonam.cmd` na própria tela. Ele ajusta o WebClient do Windows (aceitar senha em HTTP e arquivos acima de 50 MB, com confirmação de administrador uma vez) e mapeia a unidade em “Este Computador”. Manualmente: “Mapear unidade de rede” com `\\sonam\dav` (servidor no computador, porta 80) ou `\\sonam@8096\dav`. |
| macOS | Finder › Ir › Conectar ao Servidor › `http://sonam.local:8096/dav/` |
| Linux | GNOME Arquivos/KDE Dolphin com `dav://…/dav/` ou `webdav://…`, ou `mount -t davfs` |
| Celular | Gerenciadores de arquivos com WebDAV |

A unidade tem quatro pastas: **Fotos**, **Músicas** e **Vídeos** são monitoradas (o que entra nelas aparece na biblioteca; subpastas viram o gênero; renomear ou mover mantém progresso e coleções; apagar o arquivo remove da biblioteca) e **Arquivos** guarda qualquer outro arquivo (documentos, backups), fora da biblioteca. O que é enviado pelo app também vai para essas pastas, então o disco de rede e a biblioteca mostram sempre os mesmos arquivos. Na atualização, envios antigos (`DADOS/media/ID`) são movidos para a pasta do tipo, mantendo progresso e coleções; uma pasta `Biblioteca` da versão anterior continua monitorada. Arquivos copiados diretamente para `DADOS/drive/…` no servidor entram em até dois minutos.

### Tela Arquivos

Para administradores e perfis com envio, a aba **Arquivos** mostra o disco da casa como um drive: navegação por pastas com trilha (Início › Fotos › Viagem), grade com miniaturas ou lista, nova pasta, enviar (botão ou arrastar e soltar, com progresso), renomear, mover para outra pasta, baixar e excluir (com confirmação; é permanente). Tocar numa foto, música ou vídeo abre o player com os itens daquela pasta. As quatro pastas principais não podem ser renomeadas nem excluídas. A tela usa a mesma API WebDAV do disco de rede, com a sessão do app.

**Por que WebDAV e não Samba?** O SMB exige a porta 445: no Android sem root ela não pode ser aberta, no Windows ela já pertence ao próprio sistema e clientes Windows não aceitam SMB em outra porta. WebDAV usa a porta do Sonam e é aberto nativamente pelo Windows, macOS e Linux. Entra com o nome e a senha do Sonam; somente administradores e perfis com permissão de envio acessam, porque a unidade é armazenamento bruto e não aplica bloqueios de gênero/coleção. Arquivos da unidade abertos no navegador são servidos com CSP `sandbox`, sem executar scripts.

## Backup no Google Drive (opcional)

O administrador pode guardar uma cópia da biblioteca (vídeos, músicas, fotos e a pasta **Arquivos** da unidade de rede) na própria conta Google. Nada é enviado sem essa configuração.

1. **Acesso ao Google (uma vez).** No [Google Cloud Console](https://console.cloud.google.com/): crie um projeto, ative a **Google Drive API**, configure a tela de consentimento como *Externo* e **publique** o app (com a permissão usada aqui não há verificação; em modo Teste o Google desconecta a cada 7 dias). Em **Credenciais**, crie um *ID do cliente OAuth* do tipo **TVs e dispositivos de entrada limitada** e cole ID e chave secreta em **Configurações › Backup no Google Drive** (ou use `SONAM_GOOGLE_CLIENT_ID` / `SONAM_GOOGLE_CLIENT_SECRET`).
2. **Conectar a conta.** O servidor mostra um código e um QR; confirme em `google.com/device` em qualquer celular ou computador. Funciona com IP da rede, nome `.local` e TV, sem endereço de retorno.
3. **Sincronizar agora.** A sincronização é manual: envia só o que é novo ou mudou, em partes de 16 MB que retomam após quedas, e confere cada arquivo por MD5. Arquivos renomeados ou movidos aqui são reorganizados lá.

No Drive, tudo fica em **Meu Drive › Sonam**, com as mesmas pastas da unidade de rede (`Fotos/Viagens/…`, `Vídeos/Filmes/…`, `Arquivos/…`), mais um `LEIA-ME.txt`. A permissão pedida é `drive.file`: o Sonam só enxerga o que ele mesmo criou, nunca o resto do Drive.

- **Liberar espaço:** apaga daqui a cópia local dos itens com cópia conferida (por tipo, ou um item pelo menu do arquivo). Antes de apagar, cada cópia é verificada no Drive. As telas da biblioteca continuam mostrando só o que está neste servidor.
- **Página Google Drive:** mesmas telas de vídeos, músicas, fotos e arquivos, mas lendo do Drive (com aviso de que demora mais). Reproduz e abre pelo servidor, com busca no vídeo. Cada item pode ser **baixado neste aparelho**, **trazido de volta ao servidor** (volta para a mesma pasta, com progresso e coleções) ou **excluído do Drive** (vai para a lixeira do Drive). Perfis veem só o que suas regras permitem; **Arquivos** só para quem tem acesso à unidade.
- Excluir um arquivo do servidor não apaga a cópia no Drive. Itens apagados à mão no Drive aparecem como *Apagado no Drive*.
- Desconectar mantém a cópia no Drive; reconecte a **mesma** conta para voltar a abri-la. Trocar de conta só é permitido quando nada está apenas no Drive.

O modo compatível (FFmpeg) não funciona para itens que estão só no Drive: traga-os de volta antes.

## Biblioteca e perfis

Envie vídeos, músicas e fotos; crie playlists ou álbuns; abra o menu do arquivo para editar nome/gênero e associar a coleções. A grade de fotos usa miniaturas locais em cache, com limite de memória no decodificador. AVIF e imagens que não podem ser reduzidas usam o original como fallback. Não há extração automática de capas ou metadados de filmes/músicas.

A pesquisa encontra nome e gênero/categoria. A ordenação usa data de envio ou nome. Álbuns são coleções de fotos. A pesquisa não interpreta rostos, objetos, localizações ou texto da imagem.

**Visualizador de fotos:** tela cheia imersiva; deslize para os lados (ou use as setas/botões) para trocar de foto, deslize para baixo para fechar e para cima para ver os detalhes. Dois toques ou pinça dão zoom, e com zoom dá para arrastar. Um toque esconde os controles. O painel **Detalhes** (tecla `i`) mostra data e hora da câmera com fuso, câmera e lente, abertura, velocidade, ISO, distância focal, resolução, tamanho, gênero, coleções e, se a foto tiver GPS, as coordenadas e a altitude. O mapa só abre na OpenStreetMap se você tocar em “Abrir no mapa”: nenhuma localização é enviada a serviços externos. As miniaturas respeitam a orientação EXIF (fotos verticais de celular não aparecem deitadas). A data da câmera ainda não é usada para ordenar a grade.

O administrador cria contas, permite ou proíbe uploads e bloqueia gêneros ou coleções por perfil. Um arquivo em uma coleção bloqueada continua bloqueado quando aparece em outra coleção permitida. Listagem, pesquisa, player, miniatura e conversão aplicam as mesmas regras no servidor. Gêneros exigem classificação pelo administrador: **não existe detecção automática de conteúdo inadequado**.

## Reprodução e GPU

O padrão é **direct play**: transferência do original com HTTP Range. O aparelho que assiste decodifica, usando seus próprios codecs e sua aceleração quando disponíveis. Um servidor antigo consegue servir um vídeo compatível sem recodificá-lo. Isso não transfere magicamente um codec incompatível ao cliente.

Para formatos incompatíveis, instale **[FFmpeg](https://ffmpeg.org/download.html)** no PATH do servidor e selecione **Compatível** no player. A saída é MP4 fragmentado com H.264/AAC. Há no máximo uma conversão simultânea, e o processo termina quando a conexão fecha. Em caso de falha no modo GPU, tente o botão compatível novamente para forçar CPU. A conversão por CPU usa dois threads de codec, mas filtros/decodificação ainda podem usar recursos adicionais.

GPU vem habilitada como preferência. O servidor testa uma codificação real de um quadro com NVENC, QSV, AMF, VAAPI e VideoToolbox; uma listagem de encoder, sozinha, não é considerada detecção de hardware. Se nenhum teste funcionar, usa `libx264`. É possível desativar GPU em Configurações. O teste verifica **codificação**; não há garantia de decodificação zero-copy ou aceleração de todos os codecs.

O modo compatível é inicial: permite iniciar no ponto já assistido, mas não implementa HLS/DASH ou busca arbitrária eficiente durante a conversão. A retomada, busca por trechos e controles completos são mais confiáveis no original. Não há HDR tone mapping, seleção de faixas, legendas externas, Chromecast ou DLNA nesta versão. Disponibilidade de codecs varia por navegador e modelo de TV.

## Configuração do servidor

| Variável | Padrão | Finalidade |
|---|---|---|
| `SONAM_DATA` | `data` | Banco, originais e miniaturas |
| `SONAM_BIND` | `0.0.0.0:8096` | IP e porta do servidor por terminal |
| `SONAM_SETUP_TOKEN` | Aleatória por execução | Chave opcional para primeiro acesso remoto |
| `SONAM_FFMPEG` | `ffmpeg` | Caminho do executável de conversão |
| `SONAM_PUBLIC_HOST` | Vazio | Host exato autorizado, incluindo porta se houver, para proxy reverso |
| `SONAM_CLOUDFLARED` | `cloudflared` | Executável usado pelo link para fora de casa |
| `SONAM_GOOGLE_CLIENT_ID` | Vazio | ID do cliente OAuth (TV/entrada limitada) para o backup no Google Drive; substitui o salvo em Configurações |
| `SONAM_GOOGLE_CLIENT_SECRET` | Vazio | Chave secreta desse cliente |

A conexão HTTP é destinada a uma rede local confiável. Para redes compartilhadas ou acesso externo, use TLS em proxy reverso e restrição de rede. O servidor não configura firewall nem encaminhamento de portas; o acesso pela internet só existe se o administrador ativar o link Cloudflare. Tokens nativos e cookies HTTP podem ser interceptados em uma rede sem proteção. Não há promessa de invulnerabilidade: consulte [SECURITY.md](SECURITY.md).

Faça backup com o servidor parado: copie todo o diretório de dados, incluindo banco, originais e miniaturas. Não copie apenas o arquivo SQLite enquanto o banco estiver em modo WAL ativo. Exclusão de arquivo é permanente nesta versão; a interface pede confirmação.

## Desenvolvimento e verificação

```sh
npm run check
npm run build
cargo fmt --check --manifest-path server/Cargo.toml
npm test
cargo build --manifest-path server/Cargo.toml
python -X utf8 scripts/smoke.py
npm run roku:build
```

O teste HTTP usa um diretório temporário e a porta 18096. Não acessa a biblioteca real. `--serve` mantém uma biblioteca descartável para verificar a interface; as credenciais de teste são exibidas no terminal e só pertencem à instância temporária. Encerre com Ctrl+C.

O cliente usa [Svelte 5 runes](https://svelte.dev/docs/svelte/$derived), o servidor usa Axum e o desktop segue a [estrutura Tauri 2](https://v2.tauri.app/start/project-structure/). As dependências têm lockfiles. A funcionalidade do CtrlB foi consultada como referência de produto; esta implementação é nova e não depende dos seus serviços ou dados. A análise está em [docs/CTRLB.md](docs/CTRLB.md).

## Licença

AGPL-3.0-only. Consulte [LICENSE](LICENSE). Bibliotecas e ferramentas mantêm suas próprias licenças. A distribuição opcional de FFmpeg também exige observar a licença da compilação utilizada. Este repositório ainda não foi publicado em um remote.
