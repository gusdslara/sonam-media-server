<script lang="ts">
  import {
    Copy,
    Check,
    Download,
    FolderOpen,
    Library,
    RefreshCw,
    ShieldCheck
  } from '@lucide/svelte';
  import { api, copyText } from './lib';
  import type { Connect } from './lib';

  let {
    info,
    admin,
    notify,
    run
  }: {
    info: Connect;
    admin: boolean;
    notify: (text: string) => void;
    run: (fn: () => Promise<void>) => Promise<void>;
  } = $props();

  type System = 'windows' | 'mac' | 'linux' | 'mobile';
  const systems: { id: System; label: string }[] = [
    { id: 'windows', label: 'Windows' },
    { id: 'mac', label: 'macOS' },
    { id: 'linux', label: 'Linux' },
    { id: 'mobile', label: 'Celular' }
  ];
  const guess = (): System => {
    const ua = navigator.userAgent;
    if (/Android|iPhone|iPad/i.test(ua)) return 'mobile';
    if (/Mac OS X/i.test(ua)) return 'mac';
    if (/Linux|CrOS/i.test(ua)) return 'linux';
    return 'windows';
  };
  let system = $state<System>(guess());
  let copied = $state('');
  // Best address for the system being set up: Windows resolves the short
  // name ("sonam") and, when the server also answers on port 80, the
  // drive becomes simply \\sonam\dav. macOS/Linux prefer name.local.
  const shortNoPort = $derived(
    info.addresses.find((a) => a.kind === 'short' && !/:\d+$/.test(a.url))
  );
  const lanFirst = $derived(
    (system === 'windows'
      ? (shortNoPort ?? info.addresses.find((a) => a.kind === 'lan'))
      : (info.addresses.find((a) => a.kind === 'name') ??
        info.addresses.find((a) => a.kind === 'lan'))) ??
      info.addresses.find((a) => a.kind === 'domain') ??
      info.addresses[0]
  );
  let chosen = $state('');
  const base = $derived(chosen || lanFirst?.url || location.origin);
  const dav = $derived(`${base}/dav/`);
  const windowsPath = $derived.by(() => {
    const u = new URL(dav);
    const ssl = u.protocol === 'https:' ? '@SSL' : '';
    const port = u.port ? `@${u.port}` : '';
    return `\\\\${u.hostname}${ssl}${port}\\dav`;
  });
  const davScheme = $derived(dav.replace(/^http/, 'dav'));
  const linuxMount = $derived(`sudo mount -t davfs ${dav} /mnt/sonam`);

  async function copy(text: string) {
    await run(async () => {
      await copyText(text);
      copied = text;
      setTimeout(() => (copied = ''), 1800);
    });
  }
  function scan() {
    void run(async () => {
      const r = await api<{ added: number; moved: number; updated: number; removed: number }>(
        '/drive/scan',
        'POST'
      );
      notify(
        r.added || r.moved || r.updated || r.removed
          ? `Pasta verificada: ${r.added} novo(s), ${r.removed} removido(s)`
          : 'Pasta verificada: nada mudou'
      );
    });
  }
</script>

{#snippet copyRow(text: string, label: string)}
  <div class="copy-row">
    <code>{text}</code>
    <button
      class="icon-button copy-button"
      aria-label={`Copiar ${label}`}
      onclick={() => copy(text)}
      >{#if copied === text}<Check size={16} />{:else}<Copy size={16} />{/if}</button
    >
  </div>
{/snippet}

<div class="drive-folders">
  <div>
    <Library size={20} />
    <strong>{info.drive.folders.join(', ')}</strong>
    <p>
      O que você guarda aqui entra sozinho na biblioteca, e o que envia pelo app aparece aqui.
      Subpastas viram o gênero (por exemplo, Vídeos/Desenhos).
    </p>
  </div>
  <div>
    <FolderOpen size={20} />
    <strong>{info.drive.files_folder}</strong>
    <p>Documentos, backups e qualquer outro arquivo. Fica guardado em casa, fora da biblioteca.</p>
  </div>
</div>

{#if info.addresses.length > 1}
  <label class="drive-address"
    >Conectar usando
    <select bind:value={chosen}>
      <option value="">{lanFirst?.url} (recomendado)</option>
      {#each info.addresses.filter((a) => a.url !== lanFirst?.url) as a (a.url)}
        <option value={a.url}>{a.url}</option>
      {/each}
    </select>
  </label>
{/if}

<div class="segmented" role="tablist" aria-label="Sistema do computador">
  {#each systems as s (s.id)}
    <button
      role="tab"
      aria-selected={system === s.id}
      class:active={system === s.id}
      onclick={() => (system = s.id)}>{s.label}</button
    >
  {/each}
</div>

<div class="drive-steps" role="tabpanel">
  {#if system === 'windows'}
    <ol>
      <li>
        Baixe e abra o assistente. Ele libera o WebDAV do Windows para HTTP e arquivos grandes (pede
        confirmação de administrador uma única vez) e conecta a unidade.
        <a
          class="primary drive-download"
          href={`/api/drive/windows?url=${encodeURIComponent(base)}`}
          download="conectar-sonam.cmd"><Download size={17} />Baixar assistente (.cmd)</a
        >
      </li>
      <li>Digite a senha do perfil <strong>{info.drive.user}</strong> quando pedir.</li>
      <li>A unidade aparece em <strong>Este Computador</strong>, como um disco.</li>
    </ol>
    <details>
      <summary>Prefiro conectar manualmente</summary>
      <p>
        Explorador de Arquivos › Este Computador › <strong>Mapear unidade de rede</strong> › cole o
        endereço abaixo. Em HTTP, o Windows só aceita a senha depois de ajustar
        <code>BasicAuthLevel=2</code> (é o que o assistente faz). Pelo link HTTPS de fora de casa, funciona
        direto.
      </p>
      {@render copyRow(windowsPath, 'caminho para o Windows')}
      {@render copyRow(dav, 'endereço WebDAV')}
    </details>
  {:else if system === 'mac'}
    <ol>
      <li>No Finder, abra <strong>Ir › Conectar ao Servidor</strong> (⌘K).</li>
      <li>Cole o endereço e entre com nome e senha do Sonam.</li>
    </ol>
    {@render copyRow(dav, 'endereço WebDAV')}
  {:else if system === 'linux'}
    <ol>
      <li>
        GNOME Arquivos: <strong>Outros locais › Conectar ao servidor</strong>. KDE Dolphin: digite o
        endereço com <code>webdav://</code> na barra.
      </li>
      <li>Entre com nome e senha do Sonam.</li>
    </ol>
    {@render copyRow(davScheme, 'endereço para GNOME')}
    <details>
      <summary>Montar pelo terminal (davfs2)</summary>
      {@render copyRow(linuxMount, 'comando de montagem')}
    </details>
  {:else}
    <ol>
      <li>
        Use um gerenciador de arquivos com WebDAV (por exemplo, Solid Explorer, CX File Explorer ou
        Arquivos do iPhone com um app WebDAV).
      </li>
      <li>Adicione um servidor WebDAV com este endereço, seu nome e sua senha.</li>
    </ol>
    {@render copyRow(dav, 'endereço WebDAV')}
  {/if}
</div>

<div class="info-box">
  <ShieldCheck size={20} />
  <p>
    <strong>Por que WebDAV e não Samba (SMB)?</strong> O SMB exige a porta 445, que o celular não pode
    abrir sem root e que o Windows já ocupa. O WebDAV usa a mesma porta do Sonam e é aberto nativamente
    pelo Windows, macOS e Linux. Só perfis administradores ou com permissão de envio acessam a unidade.
  </p>
</div>

{#if admin}
  <button class="secondary drive-scan" onclick={scan}
    ><RefreshCw size={17} />Atualizar a biblioteca agora</button
  >
{/if}
