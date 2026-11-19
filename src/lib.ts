export type User = {
  id: string;
  name: string;
  admin: boolean;
  can_upload: boolean;
  blocked_genres: string[];
  blocked_collections: string[];
};
export type Media = {
  id: string;
  name: string;
  kind: 'video' | 'audio' | 'photo';
  mime: string;
  size: number;
  genre: string;
  created: number;
  seconds: number;
  collections: string[];
  path: string | null;
  /** Local copy removed; bytes come from Google Drive. */
  cloud_only?: boolean;
  /** Google Drive copy: matches the local file, changed since, or none. */
  backup?: 'ok' | 'outdated' | null;
  /** Item listed from Google Drive (its id is the Drive copy's id). */
  remote?: boolean;
};
export type PhotoInfo = {
  width: number | null;
  height: number | null;
  taken: string | null;
  taken_offset: string | null;
  make: string | null;
  model: string | null;
  lens: string | null;
  aperture: number | null;
  exposure: string | null;
  iso: number | null;
  focal_length: number | null;
  focal_length_35mm: number | null;
  flash: boolean | null;
  software: string | null;
  location: { latitude: number; longitude: number; altitude: number | null } | null;
};
export type MediaDetails = {
  id: string;
  name: string;
  kind: Media['kind'];
  mime: string;
  size: number;
  genre: string;
  created: number;
  path: string | null;
  collections: string[];
  photo: PhotoInfo | null;
};
export type Collection = { id: string; name: string; kind: 'video' | 'audio' | 'photo' };
export type Address = {
  kind: 'domain' | 'public' | 'short' | 'name' | 'lan' | 'vpn';
  url: string;
};
export type Tunnel = {
  available: boolean;
  enabled: boolean;
  status: { state: 'off' | 'starting' | 'on' | 'error'; url: string | null; error: string | null };
};
export type Settings = {
  gpu: boolean;
  playback: string;
  transcoding_available: boolean;
  encoder: string | null;
  port: number;
  used_bytes: number;
  data_path: string;
  name: string;
  name_error: string | null;
  addresses: Address[];
  tunnel: Tunnel;
  drive: { path: string; folders: string[]; files_folder: string };
  public_url: string;
  short: boolean;
};
export type Connect = {
  name: string;
  port: number;
  addresses: Address[];
  drive: { allowed: boolean; user: string; folders: string[]; files_folder: string };
};
/** Clipboard API needs HTTPS or localhost; LAN HTTP falls back to execCommand. */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.append(area);
    area.select();
    const ok = document.execCommand('copy');
    area.remove();
    if (!ok) throw new Error('Não foi possível copiar. Selecione o texto e copie manualmente.');
  }
}
export type DriveEntry = {
  name: string;
  path: string;
  dir: boolean;
  size: number;
  modified: number;
  media: { id: string; kind: Media['kind']; mime: string } | null;
};
export type DriveListing = {
  path: string;
  watched: boolean;
  entries: DriveEntry[];
  free: number | null;
  total: number | null;
};
export const addressLabels: Record<Address['kind'], string> = {
  domain: 'Seu domínio',
  short: 'Nome curto (Windows e Tailscale)',
  public: 'Pela internet',
  name: 'Nome na rede de casa',
  lan: 'IP na rede de casa',
  vpn: 'Rede privada (Tailscale/VPN)'
};
export async function api<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`/api${path}`, {
    method,
    credentials: 'same-origin',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body)
  });
  const data = await response
    .json()
    .catch(() => ({ error: 'O servidor não respondeu. Verifique se ele está em execução.' }));
  if (!response.ok) throw new Error(data.error || `Erro ${response.status}`);
  return data;
}
export const source = (m: Media) =>
  m.remote ? `/api/cloud/items/${m.id}/stream` : `/api/media/${m.id}/stream`;
export const thumbnail = (m: Pick<Media, 'id' | 'remote'>) =>
  m.remote ? `/api/cloud/items/${m.id}/thumbnail` : `/api/media/${m.id}/thumbnail`;
export type Tally = { count: number; bytes: number };
export type CloudItem = {
  id: string;
  media_id: string | null;
  name: string;
  kind: Media['kind'] | 'file';
  mime: string;
  size: number;
  genre: string;
  path: string;
  created: number;
  uploaded: number;
  /** A local copy exists on this server too. */
  here: boolean;
  /** Removed by hand in Google Drive. */
  lost: boolean;
  collections: string[];
};
export type CloudJob = {
  running: boolean;
  kind: '' | 'sync' | 'free' | 'restore';
  done: number;
  total: number;
  bytes_done: number;
  bytes_total: number;
  current: string;
  errors: string[];
  finished: number | null;
  summary: string;
  cancelled: boolean;
};
export type CloudStatus = {
  configured: boolean;
  managed: boolean;
  client_id: string | null;
  connected: boolean;
  email: string | null;
  folder: string | null;
  quota: { used: number | null; limit: number | null } | null;
  /** Sign-in problem (revoked access, deleted client...). */
  problem: string | null;
  /** The problem is a rejected sign-in: connecting again fixes it. */
  reauth: boolean;
  device: {
    user_code: string;
    verification_url: string;
    expires: number;
    error: string | null;
  } | null;
  job: CloudJob;
  here: Tally;
  backed: Tally;
  pending: Tally;
  only_cloud: Tally;
  lost: Tally;
  freeable: Partial<Record<CloudItem['kind'], Tally>>;
  last_sync: number | null;
};
/** Drive copy shown with the regular players and viewers. */
export const asMedia = (c: CloudItem): Media => ({
  id: c.id,
  name: c.name,
  kind: c.kind === 'file' ? 'video' : c.kind,
  mime: c.mime,
  size: c.size,
  genre: c.genre,
  created: c.created,
  seconds: 0,
  collections: c.collections,
  path: c.path,
  backup: 'ok',
  remote: true
});
export const size = (bytes: number) =>
  bytes >= 1024 ** 3
    ? `${(bytes / 1024 ** 3).toLocaleString('pt-BR', { maximumFractionDigits: 1 })} GB`
    : bytes >= 1024 ** 2
      ? `${(bytes / 1024 ** 2).toLocaleString('pt-BR', { maximumFractionDigits: 1 })} MB`
      : `${Math.max(1, Math.round(bytes / 1024))} KB`;
export const clock = (seconds: number) =>
  `${Math.floor((seconds || 0) / 60)}:${String(Math.floor((seconds || 0) % 60)).padStart(2, '0')}`;
export const labels = { video: 'Vídeos', audio: 'Músicas', photo: 'Fotos' };
