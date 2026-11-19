# Segurança

Este projeto está em desenvolvimento e ainda não recebeu auditoria independente.

## Proteções implementadas

- Senhas com Argon2id e salt aleatório; processamento de senha fora do executor assíncrono.
- Sessões com tokens aleatórios de alta entropia; somente hashes SHA-256 são armazenados no SQLite; expiração em 30 dias.
- Cookie HttpOnly/SameSite=Strict; atributo Secure quando a origem da requisição é HTTPS. O proxy deve preservar Host e Origin corretamente.
- Primeiro administrador criado em transação; acesso remoto exige chave do terminal. Um usuário local do próprio servidor pode configurar sem chave.
- PIN de seis dígitos, expiração em cinco minutos e segredo separado para consulta. Resgate único. O PIN sozinho não é uma sessão e só pode ser aprovado por usuário autenticado.
- Limite de tentativas de login, criação e aprovação de PIN por IP. O limite é em memória e reinicia com o servidor. Proxies concentram os clientes em um IP; não confiamos em X-Forwarded-For recebido arbitrariamente.
- Validação de Origin, Sec-Fetch-Site e Host para reduzir CSRF e DNS rebinding. Não há CORS aberto.
- Autorização por perfil em cada acesso a original, miniatura, conversão e posição de reprodução, além de pesquisa/listagem. Negação por coleção prevalece.
- IDs gerados pelo servidor e caminhos definidos a partir de registros internos; nomes enviados pelo usuário nunca viram caminhos de arquivo.
- SQL parametrizado. HTML, SVG, scripts e executáveis não pertencem à allowlist de containers de mídia.
- Upload em arquivo parcial, streaming para disco e publicação após validação de assinatura do container. Arquivos parciais abandonados são removidos no próximo início.
- Limites: dois uploads simultâneos, 64 GiB por requisição, 100 arquivos por requisição; uma conversão e duas gerações de miniatura simultâneas. Decodificação de imagem limitada em dimensões e memória.
- FFmpeg invocado com argumentos separados, sem shell. Conversores morrem quando o corpo HTTP é descartado; nenhum caminho ou argumento de codec vem diretamente do usuário.
- Unidade WebDAV: somente administrador ou perfis com envio; Basic auth com limite de tentativas por IP e cache de credenciais por hash; arquivos servidos com `CSP: sandbox` (HTML enviado à unidade não executa script nem herda a origem do app); caminhos guardados no banco são relativos e validados contra `..`; links simbólicos ocultos e não seguidos pelo scanner.
- Túnel para fora de casa desligado por padrão; ligado só por administrador. Requisições pelo túnel chegam por porta interna dedicada e usam o IP do visitante, nunca o loopback.
- Backup no Google Drive opcional e desligado por padrão; só o administrador configura, sincroniza, libera espaço, baixa de volta ou exclui. Permissão OAuth mínima `drive.file` (apenas arquivos criados pelo app). O refresh token e a chave secreta do cliente ficam no SQLite do servidor e nunca voltam para a interface; desconectar revoga o token no Google. A cópia local só é apagada depois de conferir tamanho, data e MD5 com a cópia existente no Drive. Arquivos do Drive são servidos pelo próprio servidor, com as mesmas regras de perfil e o mesmo CSP; o Google recebe apenas o token, nunca a sessão do Sonam.
- CSP, nosniff e proibição de frames. Aplicativo Tauri sem comandos IPC ou capabilities remotas; WebView do cliente Android sem acesso a arquivos ou ponte JavaScript nativa.

## Limites e responsabilidades

Uma assinatura de mídia válida não prova ausência de malware ou conteúdo inadequado. A allowlist não substitui antivírus nem análise de conteúdo. Mantenha FFmpeg, navegadores, WebViews e o aplicativo atualizados. Contas com upload podem consumir disco: ainda não há cotas por perfil, análise de espaço livre ou uploads retomáveis.

Gêneros/categorias são manuais. Uma conta com upload só deve ser concedida a pessoas confiáveis. Administradores têm acesso completo e podem editar classificações. Não há isolamento contra administrador malicioso ou outro usuário com acesso ao diretório de dados no sistema operacional.

HTTP local não cifra senhas ou sessões na rede; isso inclui a senha enviada pela unidade WebDAV em Basic auth. O assistente do Windows ativa `BasicAuthLevel=2`, que permite Basic em HTTP para qualquer site WebDAV naquele computador; use somente em rede confiável ou prefira o link HTTPS. Com o link Cloudflare ligado, a tela de login fica exposta na internet: use senhas fortes e desligue quando não precisar. Para uso além de uma LAN confiável, configure HTTPS e controle de rede; não exponha diretamente a porta 8096 na internet. Não há certificados automáticos ou autenticação de segundo fator nesta versão.

Clientes nativos guardam sessão para lembrar o aparelho. Atualmente não existe painel de aparelhos para revogação individual. Sair invalida a sessão atual; para incidentes, pare o servidor, remova as sessões do banco por administração local e reinicie. Alteração/recuperação de senha ainda não é oferecida pela interface.

As miniaturas em cache exigem autorização, mas o fallback de imagens não suportadas envia o original. O banco e os originais não são cifrados em repouso; com o backup ligado, o banco guarda um refresh token com acesso aos arquivos que o Sonam criou no Drive. A cópia no Google Drive segue as regras e a privacidade da conta Google, não é cifrada pelo Sonam e pode ser vista por quem tiver acesso a essa conta. Proteja o usuário do sistema e os backups. Os limites de recursos são iniciais, não uma defesa completa contra usuários autenticados hostis.

## Comunicar vulnerabilidades

Não publique arquivos pessoais, senhas, tokens, PINs ou logs completos em issues. Um canal privado de reporte deve ser configurado pelo mantenedor antes de uma publicação pública do projeto. Nenhum endereço externo de segurança é presumido aqui.
