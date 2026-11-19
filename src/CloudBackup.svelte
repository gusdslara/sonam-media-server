<script lang="ts">
  import { onMount } from 'svelte';
  import QRCode from 'qrcode';
  import {
    Cloud,
    CloudCheck,
    CloudUpload,
    CloudDownload,
    CloudAlert,
    HardDrive,
    RefreshCw,
    LoaderCircle,
    ExternalLink,
    ShieldCheck,
    Unplug,
    X,
    Check,
    Folder,
    ChevronRight,
    TriangleAlert
  } from '@lucide/svelte';
  import { api, size, clock } from './lib';
  import type { CloudStatus, CloudItem } from './lib';

  let {
    notify,
    onchanged,
    onbrowse
  }: {
    notify: (text: string) => void;
    /** Library changed (items freed or downloaded back). */
    onchanged: () => void;
    /** Open the Google Drive page of the app. */
    onbrowse: () => void;
  } = $props();

  let status = $state<CloudStatus | null>(null);
  let error = $state('');
  let working = $state(false);
  let clientId = $state('');
  let clientSecret = $state('');
  let editingConfig = $state(false);
  let qr = $state('');
  let left = $state(0);
  let confirm = $state<'' | 'free' | 'restore' | 'disconnect'>('');
  let freeKinds = $state<CloudItem['kind'][]>([]);
  let showErrors = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let wasRunning = false;

  const KINDS: { kind: CloudItem['kind']; label: string }[] = [
    { kind: 'video', label: 'Filmes e vídeos' },
    { kind: 'audio', label: 'Músicas' },
    { kind: 'photo', label: 'Fotos' },
    { kind: 'file', label: 'Arquivos' }
  ];
  const freeing = $derived(
    freeKinds.reduce(
      (t, k) => ({
        count: t.count + (status?.freeable[k]?.count ?? 0),
        bytes: t.bytes + (status?.freeable[k]?.bytes ?? 0)
      }),
      { count: 0, bytes: 0 }
    )
  );
  const job = $derived(status?.job);
  const percent = $derived(
    !job
      ? 0
      : job.bytes_total > 0
        ? (job.bytes_done / job.bytes_total) * 100
        : job.total > 0
          ? (job.done / job.total) * 100
          : 0
  );
  const quotaPercent = $derived(
    status?.quota?.used != null && status.quota.limit
      ? Math.min(100, (status.quota.used / status.quota.limit) * 100)
      : null
  );
  const jobVerb = { sync: 'Enviando', free: 'Liberando', restore: 'Baixando', '': '' };
  const when = (n: number) =>
    new Intl.DateTimeFormat('pt-BR', { dateStyle: 'short', timeStyle: 'short' }).format(n * 1000);

  async function load() {
    clearTimeout(timer);
    try {
      status = await api<CloudStatus>('/cloud');
      if (status.device && !status.device.error) {
        left = Math.max(0, status.device.expires - Math.floor(Date.now() / 1000));
        if (!qr)
          qr = await QRCode.toDataURL(status.device.verification_url, { width: 180, margin: 2 });
      } else qr = '';
      const running = status.job.running;
      if (wasRunning && !running) {
        onchanged();
        if (status.job.summary) notify(status.job.summary);
      }
      wasRunning = running;
    } catch (e) {
      error = (e as Error).message;
    }
    // Poll while something is happening (sign-in or a task).
    if (status?.job.running || (status?.device && !status.device.error))
      timer = setTimeout(load, status?.device ? 2500 : 1500);
  }
  onMount(() => {
    void load();
    return () => clearTimeout(timer);
  });

  async function run(fn: () => Promise<void>) {
    working = true;
    error = '';
    try {
      await fn();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      working = false;
    }
  }
  const saveConfig = () =>
    run(async () => {
      await api('/cloud/config', 'PUT', { client_id: clientId, client_secret: clientSecret });
      clientSecret = '';
      editingConfig = false;
      await load();
      notify('Acesso ao Google salvo');
    });
  const connect = () =>
    run(async () => {
      qr = '';
      await api('/cloud/connect', 'POST');
      await load();
    });
  const cancelConnect = () =>
    run(async () => {
      await api('/cloud/connect', 'DELETE');
      await load();
    });
  const sync = () =>
    run(async () => {
      await api('/cloud/sync', 'POST');
      await load();
    });
  const cancelJob = () =>
    run(async () => {
      await api('/cloud/cancel', 'POST');
      await load();
    });
  const free = () =>
    run(async () => {
      await api('/cloud/free', 'POST', { kinds: freeKinds });
      confirm = '';
      freeKinds = [];
      await load();
    });
  const restoreAll = () =>
    run(async () => {
      await api('/cloud/restore', 'POST', { all: true });
      confirm = '';
      await load();
    });
  const disconnect = () =>
    run(async () => {
      await api('/cloud/disconnect', 'POST');
      confirm = '';
      await load();
      onchanged();
      notify('Conta Google desconectada');
    });
</script>

<span class="modal-icon"><Cloud /></span>
<span class="eyebrow">UMA CÓPIA FORA DE CASA</span>
<h2>Backup no Google Drive.</h2>
<p>
  Opcional. Guarda uma cópia da sua biblioteca na sua conta Google, nas mesmas pastas da unidade de
  rede. O Sonam continua mostrando o que está neste servidor; o que estiver só no Drive aparece
  na página <strong>Google Drive</strong>.
</p>

{#if !status}
  <div class="cloud-loading"><LoaderCircle class="spin" size={20} />Consultando…</div>
{:else if !status.configured || editingConfig}
  <!-- One-time setup of the owner's own Google OAuth client. -->
  <ol class="pair-steps compact cloud-steps">
    <li>
      <span>1</span>
      <p>
        No <a href="https://console.cloud.google.com/" target="_blank" rel="noreferrer noopener"
          >Google Cloud Console<ExternalLink size={12} /></a
        >, crie um projeto e ative a <strong>Google Drive API</strong>.
      </p>
    </li>
    <li>
      <span>2</span>
      <p>
        Em <strong>Tela de consentimento OAuth</strong>, escolha <em>Externo</em> e depois
        <strong>Publicar app</strong>. Com a permissão usada aqui não há verificação; em modo Teste
        o Google desconecta a cada 7 dias.
      </p>
    </li>
    <li>
      <span>3</span>
      <p>
        Em <strong>Credenciais › Criar credenciais › ID do cliente OAuth</strong>, escolha o tipo
        <strong>TVs e dispositivos de entrada limitada</strong>. Copie o ID e a chave secreta.
      </p>
    </li>
  </ol>
  {#if status.managed}
    <p class="small muted">
      O ID do cliente vem da variável <code>SONAM_GOOGLE_CLIENT_ID</code> deste servidor.
    </p>
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void saveConfig();
      }}
    >
      <label
        >ID do cliente<input
          required
          autocomplete="off"
          spellcheck="false"
          bind:value={clientId}
          placeholder="123456-abc.apps.googleusercontent.com"
        /></label
      ><label
        >Chave secreta do cliente<input
          required
          type="password"
          autocomplete="off"
          bind:value={clientSecret}
          placeholder="GOCSPX-…"
        /><small>Fica só neste servidor. Nunca é mostrada de novo.</small></label
      >
      <button class="primary" disabled={working}
        >{#if working}<LoaderCircle size={17} class="spin" />{/if}Salvar acesso<Check
          size={17}
        /></button
      >
      {#if editingConfig}<button
          type="button"
          class="text-button"
          onclick={() => (editingConfig = false)}>Cancelar</button
        >{/if}
    </form>
  {/if}
  <div class="info-box">
    <ShieldCheck size={20} />
    <p>
      <strong>Permissão mínima.</strong> O Sonam pede só acesso aos arquivos que ele mesmo cria
      (<code>drive.file</code>). Ele não enxerga nem altera o resto do seu Drive.
    </p>
  </div>
{:else if status.device}
  <!-- Device sign-in: same pattern as pairing a TV. -->
  <div class="cloud-device">
    {#if status.device.error}
      <div class="error" role="alert">{status.device.error}</div>
      <button class="primary" disabled={working} onclick={connect}
        ><RefreshCw size={17} />Gerar outro código</button
      >
      <button class="text-button" onclick={cancelConnect}>Cancelar</button>
    {:else}
      <ol class="pair-steps compact">
        <li>
          <span>1</span>
          <p>
            Em qualquer celular ou computador, abra <strong
              >{status.device.verification_url.replace(/^https?:\/\/(www\.)?/, '')}</strong
            > (ou leia o QR).
          </p>
        </li>
        <li>
          <span>2</span>
          <p>Entre na conta Google onde quer guardar a cópia e digite o código abaixo.</p>
        </li>
      </ol>
      <div class="pair-card">
        {#if qr}<img
            class="qr"
            src={qr}
            alt="QR code que abre a página de autorização do Google"
          />{/if}
        <div>
          <span class="pair-label">Código do Google</span>
          <strong class="pair-pin cloud-code">{status.device.user_code}</strong>
          <span class="pair-wait"
            ><LoaderCircle size={14} class="spin" /> Aguardando você confirmar · expira em {clock(
              left
            )}</span
          >
        </div>
      </div>
      <a
        class="secondary cloud-link"
        href={status.device.verification_url}
        target="_blank"
        rel="noreferrer noopener"><ExternalLink size={16} />Abrir a página do Google aqui</a
      >
      <button class="text-button" onclick={cancelConnect}>Cancelar</button>
    {/if}
  </div>
{:else if !status.connected}
  {#if status.email}
    <p class="cloud-account-note">
      <TriangleAlert size={16} />Desconectado. Para continuar usando a cópia existente, conecte de
      novo a conta <strong>{status.email}</strong>.
    </p>
  {/if}
  <button class="primary cloud-connect" disabled={working} onclick={connect}
    >{#if working}<LoaderCircle size={17} class="spin" />{:else}<Cloud size={18} />{/if}Conectar
    conta Google</button
  >
  <p class="small muted cloud-hint">
    Aparece um código; você confirma no celular. Não é preciso digitar senha aqui.
  </p>
  {#if !status.managed}<button class="text-button" onclick={() => (editingConfig = true)}
      >Trocar o ID do cliente Google</button
    >{/if}
{:else}
  {#if status.problem}
    <div class="cloud-account-note cloud-problem" role="alert">
      <TriangleAlert size={16} />
      <div>
        <strong
          >{status.reauth
            ? 'O acesso ao Google Drive não está funcionando.'
            : 'Não foi possível falar com o Google Drive agora.'}</strong
        >
        <span>{status.problem}</span>
        {#if status.reauth}<button class="secondary" disabled={working} onclick={connect}
            ><RefreshCw size={15} />Conectar de novo</button
          >{/if}
      </div>
    </div>
  {/if}
  <div class="cloud-account">
    <span class="cloud-avatar"><Cloud size={20} /></span>
    <div>
      <strong>{status.email || 'Conta Google'}</strong>
      {#if status.quota?.used != null}
        <small
          >{size(status.quota.used)} usados{status.quota.limit
            ? ` de ${size(status.quota.limit)}`
            : ''} no Google Drive</small
        >
        {#if quotaPercent !== null}<span class="cloud-quota" aria-hidden="true"
            ><i style={`width:${quotaPercent}%`}></i></span
          >{/if}
      {:else}<small>{status.reauth ? 'Precisa conectar de novo' : 'Conectado'}</small>{/if}
    </div>
    {#if status.folder}<a
        class="icon-button"
        href={`https://drive.google.com/drive/folders/${status.folder}`}
        target="_blank"
        rel="noreferrer noopener"
        aria-label="Abrir a pasta Sonam no Google Drive"
        title="Abrir no Google Drive"><ExternalLink size={17} /></a
      >{/if}
  </div>

  <div class="cloud-stats">
    <div>
      <HardDrive size={17} /><strong>{status.here.count}</strong><span>neste servidor</span><small
        >{size(status.here.bytes || 0)}</small
      >
    </div>
    <div class="ok">
      <CloudCheck size={17} /><strong>{status.backed.count}</strong><span>com cópia no Drive</span
      ><small>{size(status.backed.bytes || 0)}</small>
    </div>
    <div class:warn={status.pending.count > 0}>
      <CloudUpload size={17} /><strong>{status.pending.count}</strong><span>ainda sem cópia</span
      ><small>{size(status.pending.bytes || 0)}</small>
    </div>
    <button class="cloud-only" onclick={onbrowse} disabled={!status.only_cloud.count}>
      <Cloud size={17} /><strong>{status.only_cloud.count}</strong><span>só no Google Drive</span
      ><small>{size(status.only_cloud.bytes || 0)}</small>
    </button>
  </div>
  {#if status.lost.count}
    <p class="cloud-account-note">
      <CloudAlert size={16} />{status.lost.count} item(ns) foram apagados direto no Google Drive e não
      existem mais neste servidor. Veja na página Google Drive.
    </p>
  {/if}

  {#if job && (job.running || job.finished)}
    <div class="cloud-job" class:done={!job.running} role="status">
      <div class="cloud-job-head">
        {#if job.running}<LoaderCircle
            size={18}
            class="spin"
          />{:else if job.errors.length}<TriangleAlert size={18} />{:else}<Check size={18} />{/if}
        <div>
          <strong
            >{job.running
              ? `${jobVerb[job.kind]}${job.total ? ` ${Math.min(job.done + 1, job.total)} de ${job.total}` : ''}`
              : job.cancelled
                ? 'Cancelado'
                : job.summary}</strong
          >
          <small
            >{job.running
              ? job.current
              : job.finished
                ? `Terminou em ${when(job.finished)}`
                : ''}</small
          >
        </div>
        {#if job.running}<button class="secondary" disabled={working} onclick={cancelJob}
            ><X size={15} />Cancelar</button
          >{/if}
      </div>
      {#if job.running}
        <progress max="100" value={percent}></progress>
        {#if job.bytes_total}<small class="muted"
            >{size(job.bytes_done)} de {size(job.bytes_total)}</small
          >{/if}
      {/if}
      {#if job.errors.length}
        <button class="text-button" onclick={() => (showErrors = !showErrors)}
          >{job.errors.length} problema(s){showErrors ? ' · ocultar' : ' · ver'}</button
        >
        {#if showErrors}<ul class="cloud-errors">
            {#each job.errors as e, i (i)}<li>{e}</li>{/each}
          </ul>{/if}
      {/if}
    </div>
  {/if}

  <div class="cloud-actions">
    <button class="primary" disabled={working || job?.running} onclick={sync}
      ><RefreshCw size={17} />Sincronizar agora</button
    >
    <small class="muted"
      >{status.last_sync
        ? `Última sincronização: ${when(status.last_sync)}`
        : 'Nenhuma sincronização ainda'} · só envia o que é novo ou mudou</small
    >
  </div>

  <section class="cloud-section">
    <h3 class="subheading">Liberar espaço neste servidor</h3>
    <p class="small muted">
      Apaga a cópia daqui dos itens que já têm cópia conferida no Drive. Eles somem das telas do
      servidor e continuam na página Google Drive, de onde dá para assistir, abrir ou baixar de
      volta.
    </p>
    {#if confirm === 'free'}
      <div class="cloud-confirm" role="alertdialog" aria-label="Confirmar liberação de espaço">
        <strong>Apagar daqui {freeing.count} item(ns), {size(freeing.bytes)}?</strong>
        <p>
          Antes de apagar, cada arquivo é conferido no Google Drive. Arquivos que mudaram depois da
          cópia ficam aqui.
        </p>
        <div>
          <button class="primary danger-bg" disabled={working} onclick={free}
            >{#if working}<LoaderCircle size={17} class="spin" />{/if}Liberar {size(
              freeing.bytes
            )}</button
          ><button class="text-button" onclick={() => (confirm = '')}>Manter aqui</button>
        </div>
      </div>
    {:else}
      <div class="cloud-kinds">
        {#each KINDS as k (k.kind)}
          {@const t = status.freeable[k.kind]}
          <label class="checkbox-label" class:empty={!t?.count}
            ><input
              type="checkbox"
              value={k.kind}
              disabled={!t?.count}
              bind:group={freeKinds}
            /><span>{k.label}</span><small
              >{t?.count ? `${t.count} · ${size(t.bytes)}` : 'nada com cópia'}</small
            ></label
          >
        {/each}
      </div>
      <button
        class="secondary"
        disabled={!freeing.count || job?.running}
        onclick={() => (confirm = 'free')}
        ><HardDrive size={16} />Liberar {freeing.count ? size(freeing.bytes) : 'espaço'}</button
      >
    {/if}
  </section>

  {#if status.only_cloud.count}
    <section class="cloud-section">
      <h3 class="subheading">Trazer de volta</h3>
      {#if confirm === 'restore'}
        <div class="cloud-confirm" role="alertdialog" aria-label="Confirmar download">
          <strong
            >Baixar {status.only_cloud.count} item(ns), {size(status.only_cloud.bytes)}, para este
            servidor?</strong
          >
          <p>Eles voltam para as mesmas pastas e reaparecem na biblioteca.</p>
          <div>
            <button class="primary" disabled={working} onclick={restoreAll}
              ><CloudDownload size={17} />Baixar tudo</button
            ><button class="text-button" onclick={() => (confirm = '')}>Agora não</button>
          </div>
        </div>
      {:else}
        <div class="cloud-row-actions">
          <button class="secondary" onclick={onbrowse}
            ><Cloud size={16} />Ver o que está só no Drive<ChevronRight size={16} /></button
          ><button class="secondary" disabled={job?.running} onclick={() => (confirm = 'restore')}
            ><CloudDownload size={16} />Baixar tudo de volta</button
          >
        </div>
      {/if}
    </section>
  {/if}

  <div class="info-box cloud-where">
    <Folder size={20} />
    <p>
      <strong>Onde fica no Drive:</strong> Meu Drive › {'Sonam'} › Fotos, Músicas, Vídeos e Arquivos,
      com as mesmas subpastas daqui. Dá para abrir pelo app do Google Drive também. Evite apagar por lá
      o que não estiver mais neste servidor.
    </p>
  </div>

  {#if confirm === 'disconnect'}
    <div class="cloud-confirm" role="alertdialog" aria-label="Confirmar desconexão">
      <strong>Desconectar {status.email}?</strong>
      <p>
        A cópia continua no seu Google Drive.{status.only_cloud.count
          ? ` ${status.only_cloud.count} item(ns) que estão só lá ficam indisponíveis até você conectar esta mesma conta de novo.`
          : ''}
      </p>
      <div>
        <button class="primary danger-bg" disabled={working} onclick={disconnect}
          ><Unplug size={17} />Desconectar</button
        ><button class="text-button" onclick={() => (confirm = '')}>Manter conectado</button>
      </div>
    </div>
  {:else}
    <button
      class="text-button danger"
      disabled={job?.running}
      onclick={() => (confirm = 'disconnect')}><Unplug size={16} />Desconectar conta Google</button
    >
  {/if}
{/if}
{#if error}<div class="error" role="alert">{error}</div>{/if}
