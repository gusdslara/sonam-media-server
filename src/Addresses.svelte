<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Copy, Check, Globe, Wifi, LoaderCircle, Pencil, TriangleAlert } from '@lucide/svelte';
  import QRCode from 'qrcode';
  import { api, copyText, addressLabels } from './lib';
  import type { Settings } from './lib';

  let {
    settings = $bindable(),
    notify,
    run
  }: {
    settings: Settings;
    notify: (text: string) => void;
    run: (fn: () => Promise<void>) => Promise<void>;
  } = $props();

  let selected = $state('');
  let qr = $state('');
  let copied = $state('');
  let editingName = $state(false);
  let draftName = $state('');
  let poll: ReturnType<typeof setInterval> | undefined;

  const shown = $derived(
    settings.addresses.find((a) => a.url === selected) ?? settings.addresses[0] ?? null
  );
  const tunnelState = $derived(settings.tunnel.status.state);

  $effect(() => {
    const url = shown?.url;
    if (!url) {
      qr = '';
      return;
    }
    void QRCode.toDataURL(url, {
      margin: 2,
      width: 220,
      color: { dark: '#17231e', light: '#ffffff' }
    }).then((data) => (qr = data));
  });

  async function refresh() {
    const next = await api<Settings>('/settings');
    settings.addresses = next.addresses;
    settings.public_url = next.public_url;
    settings.short = next.short;
    settings.tunnel = next.tunnel;
    settings.name = next.name;
    settings.name_error = next.name_error;
    return next;
  }

  function watchTunnel() {
    clearInterval(poll);
    poll = setInterval(async () => {
      try {
        const next = await refresh();
        if (next.tunnel.status.state !== 'starting') {
          clearInterval(poll);
          if (next.tunnel.status.url) {
            selected = next.tunnel.status.url;
            notify('Link pela internet pronto');
          }
        }
      } catch {
        clearInterval(poll);
      }
    }, 2000);
  }
  onMount(() => {
    if (settings.tunnel.status.state === 'starting') watchTunnel();
  });
  onDestroy(() => clearInterval(poll));

  async function copy(url: string) {
    await run(async () => {
      await copyText(url);
      copied = url;
      setTimeout(() => (copied = ''), 1800);
    });
  }

  function setTunnel(on: boolean) {
    void run(async () => {
      await api('/settings', 'PUT', { tunnel: on });
      await refresh();
      if (on) watchTunnel();
      else {
        clearInterval(poll);
        selected = '';
        notify('Link pela internet desligado');
      }
    });
  }

  let editingDomain = $state(false);
  let draftDomain = $state('');
  function saveDomain(e: SubmitEvent) {
    e.preventDefault();
    void run(async () => {
      await api('/settings', 'PUT', { public_url: draftDomain.trim() });
      const next = await refresh();
      settings.public_url = next.public_url;
      editingDomain = false;
      notify(draftDomain.trim() ? 'Domínio salvo' : 'Domínio removido');
    });
  }

  function saveName(e: SubmitEvent) {
    e.preventDefault();
    void run(async () => {
      await api('/settings', 'PUT', { name: draftName });
      await refresh();
      editingName = false;
      selected = '';
      notify('Nome na rede atualizado');
    });
  }
</script>

<section class="connection-panel addresses" aria-labelledby="addresses-title">
  <div class="addresses-main">
    <span class="local-badge"><Wifi size={15} />ENDEREÇOS DA SUA CASA</span>
    <h3 id="addresses-title">Abra em qualquer tela</h3>
    {#if shown}
      <p class="server-address">{shown.url}</p>
    {/if}
    <ul class="address-list">
      {#each settings.addresses as a (a.url)}
        <li class:active={shown?.url === a.url}>
          <button
            class="address-pick"
            aria-pressed={shown?.url === a.url}
            onclick={() => (selected = a.url)}
          >
            <span class="address-kind">{addressLabels[a.kind]}</span>
            <span class="address-url">{a.url.replace(/^https?:\/\//, '')}</span>
          </button>
          <button
            class="icon-button copy-button"
            aria-label={`Copiar ${a.url}`}
            onclick={() => copy(a.url)}
            >{#if copied === a.url}<Check size={16} />{:else}<Copy size={16} />{/if}</button
          >
        </li>
      {/each}
    </ul>
    <p>
      Prefira os nomes: não mudam quando o roteador troca o IP. <strong>{settings.name}</strong>
      abre no Windows; <strong>{settings.name}.local</strong> no Mac, iPhone, Linux e na maioria dos
      Android. TVs e aparelhos antigos: use o IP ou o QR.
      {#if !settings.short}Sem <code>:{settings.port}</code> só quando o servidor roda num computador
        (porta 80 livre); no celular a porta continua.{/if}
    </p>
  </div>
  {#if qr && shown}<img class="qr" src={qr} alt={`QR code para ${shown.url}`} />{/if}
</section>

<div class="setting-row">
  <div>
    <strong>Nome na rede</strong>
    {#if editingName}
      <form class="inline-form" onsubmit={saveName}>
        <label class="name-field"
          ><span class="sr-only">Nome do servidor</span><input
            required
            maxlength="40"
            pattern="[a-z0-9]([a-z0-9\-]*[a-z0-9])?"
            autocapitalize="off"
            spellcheck="false"
            bind:value={draftName}
          /></label
        >
        <button class="primary small-button">Salvar</button>
        <button type="button" class="secondary small-button" onclick={() => (editingName = false)}
          >Cancelar</button
        >
      </form>
      <p>Letras minúsculas, números e hífen. Use nomes diferentes se tiver mais de um servidor.</p>
    {:else}
      <p>
        http://{settings.name}{settings.short ? '' : `:${settings.port}`} · http://{settings.name}.local{settings.short
          ? ''
          : `:${settings.port}`}
        {#if settings.name_error}<span class="danger"
            >· não foi possível anunciar: {settings.name_error}</span
          >{/if}
      </p>
    {/if}
  </div>
  {#if !editingName}
    <button
      class="secondary small-button"
      onclick={() => {
        draftName = settings.name;
        editingName = true;
      }}><Pencil size={15} />Alterar</button
    >
  {/if}
</div>

<div class="setting-row">
  <div>
    <strong>Seu domínio (IP público ou DDNS)</strong>
    {#if editingDomain}
      <form class="inline-form" onsubmit={saveDomain}>
        <label class="name-field wide-field"
          ><span class="sr-only">Endereço público</span><input
            maxlength="260"
            autocapitalize="off"
            spellcheck="false"
            inputmode="url"
            placeholder="https://minhacasa.duckdns.org"
            bind:value={draftDomain}
          /></label
        >
        <button class="primary small-button">Salvar</button>
        <button type="button" class="secondary small-button" onclick={() => (editingDomain = false)}
          >Cancelar</button
        >
      </form>
      <p>
        Nomes como <code>{settings.name}</code> e <code>{settings.name}.local</code> só existem
        dentro de casa. Para entrar de fora pelo seu IP público, use um nome DNS de verdade (DuckDNS
        e No-IP são gratuitos) apontando para o roteador, encaminhe a porta {settings.port} para este
        servidor e, de preferência, coloque HTTPS na frente. Deixe vazio para remover.
      </p>
    {:else}
      <p>
        {#if settings.public_url}<strong>{settings.public_url}</strong> · aceito pelo servidor e mostrado
          nos endereços.{:else}Opcional. Para quem tem IP público ou domínio próprio (ex.: DuckDNS)
          com a porta encaminhada no roteador.{/if}
      </p>
    {/if}
  </div>
  {#if !editingDomain}
    <button
      class="secondary small-button"
      onclick={() => {
        draftDomain = settings.public_url;
        editingDomain = true;
      }}><Pencil size={15} />{settings.public_url ? 'Alterar' : 'Configurar'}</button
    >
  {/if}
</div>

<div class="setting-row">
  <div>
    <strong><Globe size={15} class="inline-icon" /> Link para acessar fora de casa</strong>
    {#if !settings.tunnel.available}
      <p>
        Instale o <strong>cloudflared</strong> neste servidor para gerar um link HTTPS com um
        clique, ou use o <strong>Tailscale</strong> nos seus aparelhos (gratuito): o endereço “Rede privada”
        aparece aqui sozinho e funciona de qualquer lugar sem expor nada na internet.
      </p>
    {:else if tunnelState === 'starting'}
      <p class="tunnel-status"><LoaderCircle size={14} class="spin" /> Gerando link seguro…</p>
    {:else if tunnelState === 'on' && settings.tunnel.status.url}
      <p class="tunnel-status">
        Ativo: <strong>{settings.tunnel.status.url}</strong>. O endereço muda quando o servidor
        reinicia.
      </p>
    {:else if tunnelState === 'error'}
      <p class="danger">{settings.tunnel.status.error}</p>
    {:else}
      <p>
        Cria um endereço HTTPS público (Cloudflare) sem configurar o roteador. Qualquer pessoa com o
        link vê a tela de login: use senhas fortes.
      </p>
    {/if}
  </div>
  {#if settings.tunnel.available}
    <input
      type="checkbox"
      role="switch"
      aria-label="Link para acessar fora de casa"
      checked={settings.tunnel.enabled}
      onchange={(e) => {
        settings.tunnel.enabled = e.currentTarget.checked;
        setTunnel(e.currentTarget.checked);
      }}
    />
  {/if}
</div>
{#if settings.tunnel.enabled && settings.tunnel.available}
  <div class="info-box warning-box">
    <TriangleAlert size={20} />
    <p>
      <strong>Seu servidor está acessível pela internet.</strong> O tráfego é cifrado pela
      Cloudflare e o login continua obrigatório, com limite de tentativas. Desligue quando não
      precisar. Links rápidos não têm garantia de disponibilidade; para um endereço fixo, use
      Tailscale ou um túnel nomeado com domínio próprio (<code>SONAM_PUBLIC_HOST</code>).
    </p>
  </div>
{/if}
