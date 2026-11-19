"""End-to-end checks against a disposable, isolated server. Never uses real library data."""
import argparse
import base64
import http.cookiejar
import io
import json
import os
import shutil
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import wave
import zlib

parser = argparse.ArgumentParser()
parser.add_argument('--serve', action='store_true', help='Keep the disposable UI fixture alive until Ctrl+C')
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
binary = root / 'server/target/debug' / ('sonam-server.exe' if os.name == 'nt' else 'sonam-server')
base = 'http://127.0.0.1:18096'

def client():
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

def request(c, route, method='GET', data=None, headers=None, expect=200):
    h = {'Origin': base, **(headers or {})}
    if data is not None and not isinstance(data, bytes):
        data = json.dumps(data).encode()
        h['Content-Type'] = 'application/json'
    req = urllib.request.Request(base + '/api' + route, data=data, method=method, headers=h)
    try:
        response = c.open(req, timeout=20)
    except urllib.error.HTTPError as e:
        response = e
    body = response.read()
    assert response.code == expect, (route, response.code, body[:300])
    return json.loads(body) if 'application/json' in response.headers.get('Content-Type', '') else body

def upload(c, name, content, expect=200):
    boundary = 'sonam-test-boundary'
    body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\nContent-Type: application/octet-stream\r\n\r\n').encode() + content + f'\r\n--{boundary}--\r\n'.encode()
    return request(c, '/media', 'POST', body, {'Content-Type': f'multipart/form-data; boundary={boundary}'}, expect)

with tempfile.TemporaryDirectory(prefix='sonam-qa-') as data:
    env = {**os.environ, 'SONAM_DATA': data, 'SONAM_BIND': '127.0.0.1:18096', 'SONAM_PORT80': 'off'}
    with open(Path(data) / 'server.log', 'w', encoding='utf-8') as log:
        proc = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            anonymous, owner, viewer, tv = client(), client(), client(), client()
            for _ in range(100):
                try:
                    request(anonymous, '/status')
                    break
                except (OSError, AssertionError):
                    if proc.poll() is not None:
                        raise RuntimeError('Test server failed to start; check port 18096')
                    time.sleep(.1)
            request(anonymous, '/media', expect=401)
            request(owner, '/setup', 'POST', {'name': 'Teste local', 'password': 'sonam-test-only'})
            request(owner, '/setup', 'POST', {'name': 'Intruso', 'password': 'another-test-only'}, expect=409)
            request(owner, '/media', 'POST', b'<html>fake.mp4</html>', {'Content-Type': 'text/plain'}, expect=400)
            upload(owner, 'ataque.mp4', b'<html><script>alert(1)</script></html>', expect=400)
            def chunk(kind, value):
                return struct.pack('>I', len(value)) + kind + value + struct.pack('>I', zlib.crc32(kind + value))
            png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 32, 32, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress((b'\x00' + b'\x8a\xaa\x83' * 32) * 32)) + chunk(b'IEND', b'')
            photo = upload(owner, 'Memoria de viagem.png', png)[0]
            assert request(owner, f'/media/{photo["id"]}/thumbnail').startswith(b'\xff\xd8\xff')
            assert request(owner, f'/media/{photo["id"]}/thumbnail').startswith(b'\xff\xd8\xff')
            sound = io.BytesIO()
            with wave.open(sound, 'wb') as w:
                w.setnchannels(1); w.setsampwidth(2); w.setframerate(8000)
                w.writeframes(struct.pack('<h', 0) * 40000)
            audio = upload(owner, 'Domingo tranquilo.wav', sound.getvalue())[0]
            assert request(owner, f'/media/{photo["id"]}/stream', headers={'Range': 'bytes=0-7'}, expect=206) == png[:8]
            request(owner, f'/media/{photo["id"]}/stream', headers={'Range': 'bytes=99999-'}, expect=416)
            collection = request(owner, '/collections', 'POST', {'name': 'Viagens em familia', 'kind': 'photo'})
            request(owner, f'/collections/{collection["id"]}/items/{photo["id"]}', 'PUT')
            request(owner, f'/collections/{collection["id"]}/items/{audio["id"]}', 'PUT', expect=400)
            request(owner, f'/media/{audio["id"]}', 'PUT', {'name': audio['name'], 'genre': 'Jazz'})
            request(owner, '/users', 'POST', {'name': 'Visitante', 'password': 'viewer-test-only', 'can_upload': False, 'blocked_genres': ['jazz'], 'blocked_collections': [collection['id']]})
            request(viewer, '/login', 'POST', {'name': 'Visitante', 'password': 'viewer-test-only'})
            assert request(viewer, '/media') == []
            assert request(viewer, '/media?q=viagem') == []
            for m in [photo, audio]:
                request(viewer, f'/media/{m["id"]}/stream', expect=404)
                request(viewer, f'/media/{m["id"]}/compatible', expect=404)
                request(viewer, f'/media/{m["id"]}/thumbnail', expect=404)
                request(viewer, f'/media/{m["id"]}/progress', 'PUT', {'seconds': 2}, expect=404)
            upload(viewer, 'foto.png', png, expect=403)
            request(viewer, '/settings', expect=403)
            request(viewer, '/users', expect=403)
            request(owner, f'/media/{audio["id"]}/progress', 'PUT', {'seconds': 0.5})
            assert next(m for m in request(owner, '/media') if m['id'] == audio['id'])['seconds'] == .5
            pairing = request(tv, '/pair', 'POST')
            assert not request(tv, '/pair/' + pairing['id'], 'POST')['approved']
            request(viewer, '/pair/approve', 'POST', {'pin': pairing['pin']})
            paired = request(tv, '/pair/' + pairing['id'], 'POST')
            assert paired['approved'] and paired['token']
            assert request(tv, '/me')['name'] == 'Visitante'
            request(tv, '/pair/' + pairing['id'], 'POST', expect=410)
            assert request(tv, '/media') == []
            request(anonymous, '/pair/approve', 'POST', {'pin': pairing['pin']}, expect=401)
            request(owner, '/settings', 'PUT', {'gpu': False})
            assert request(owner, '/settings')['gpu'] is False
            request(owner, '/settings', 'PUT', {'gpu': True})
            ffmpeg = os.environ.get('SONAM_FFMPEG') or shutil.which('ffmpeg')
            if ffmpeg:
                sample = Path(data) / 'sample.mp4'
                subprocess.run([ffmpeg, '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc=size=160x90:rate=10', '-f', 'lavfi', '-i', 'sine=frequency=440', '-t', '2', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', str(sample)], check=True, timeout=30)
                video = upload(owner, 'Sessao de teste.mp4', sample.read_bytes())[0]
                for mode in ['cpu=true', 'cpu=false']:
                    converted = request(owner, f'/media/{video["id"]}/compatible?{mode}')
                    assert converted[4:8] == b'ftyp', ('invalid converted container', mode)
                    output = Path(data) / 'converted.mp4'
                    output.write_bytes(converted)
                    subprocess.run([ffmpeg, '-hide_banner', '-loglevel', 'error', '-i', str(output), '-f', 'null', '-'], check=True, timeout=30)
                print('PASS: FFmpeg CPU conversion and GPU-preferred/fallback output both decoded successfully.')
            else:
                print('SKIP: FFmpeg is not installed; conversion runtime checks were not executed.')
            # Network drive (WebDAV), type folders and the app's file browser.
            def dav(method, path, auth=None, data=None, headers=None, expect=None):
                h = dict(headers or {})
                if auth:
                    h['Authorization'] = 'Basic ' + base64.b64encode(auth.encode()).decode()
                req = urllib.request.Request(base + '/dav' + path, data=data, method=method, headers=h)
                try:
                    response = urllib.request.urlopen(req, timeout=20)
                except urllib.error.HTTPError as e:
                    response = e
                body = response.read()
                if expect is not None:
                    assert response.code == expect, (method, path, response.code, body[:200])
                return response, body
            owner_auth = 'Teste local:sonam-test-only'
            dav('PROPFIND', '/', expect=401)
            dav('PROPFIND', '/', 'Teste local:senha-errada', expect=401)
            dav('PROPFIND', '/', 'Visitante:viewer-test-only', expect=403)
            _, listing = dav('PROPFIND', '/', owner_auth, headers={'Depth': '1'}, expect=207)
            assert b'/dav/Fotos/' in listing and b'/dav/Arquivos/' in listing and b'/dav/M%C3%BAsicas/' in listing, listing[:500]
            dav('MKCOL', '/Fotos/Ferias', owner_auth, expect=201)
            dav('PUT', '/Fotos/Ferias/praia.png', owner_auth, png, expect=201)
            dav('PUT', '/Fotos/Ferias/notas.txt', owner_auth, b'nao e midia', expect=201)
            response, _ = dav('PUT', '/Arquivos/pagina.html', owner_auth, b'<script>alert(1)</script>', expect=201)
            response, _ = dav('GET', '/Arquivos/pagina.html', owner_auth, expect=200)
            assert response.headers['Content-Security-Policy'].startswith('sandbox'), 'drive files must be sandboxed'
            def drive_media():
                return [m for m in request(owner, '/media') if 'praia' in (m.get('path') or '')]
            for _ in range(80):
                found = drive_media()
                if found:
                    break
                time.sleep(.1)
            assert len(found) == 1 and found[0]['path'] == 'Fotos/Ferias/praia.png' and found[0]['genre'] == 'Ferias', found
            assert request(owner, f'/media/{found[0]["id"]}/stream') == png
            dav('MOVE', '/Fotos/Ferias/praia.png', owner_auth, headers={'Destination': base + '/dav/Fotos/praia-2026.png'}, expect=201)
            moved = request(owner, '/drive/scan', 'POST')
            assert moved['moved'] == 1, moved
            assert drive_media()[0]['id'] == found[0]['id'], 'move keeps the library entry'
            request(owner, f'/media/{found[0]["id"]}', 'DELETE')
            dav('PROPFIND', '/Fotos/praia-2026.png', owner_auth, expect=404)
            # App uploads live in the drive too, and the file browser lists them.
            assert photo['id'] in [m['id'] for m in request(owner, '/media') if (m.get('path') or '').startswith('Fotos/')]
            listing = request(owner, '/drive/list?path=Fotos')
            names = [e['name'] for e in listing['entries']]
            assert 'Ferias' in names and 'Memoria de viagem.png' in names and listing['watched'], names
            assert next(e for e in listing['entries'] if e['name'] == 'Ferias')['dir']
            assert next(e for e in listing['entries'] if e['name'] == 'Memoria de viagem.png')['media']['id'] == photo['id']
            request(owner, '/drive/list?path=..', expect=400)
            request(viewer, '/drive/list?path=Fotos', expect=403)
            dav('DELETE', '/Fotos', owner_auth, expect=403)
            dav('MOVE', '/M%C3%BAsicas', owner_auth, headers={'Destination': base + '/dav/Arquivos/x'}, expect=403)
            connect = request(owner, '/connect')
            assert connect['drive']['allowed'] and connect['addresses'], connect
            assert not request(viewer, '/connect')['drive']['allowed']
            request(owner, '/drive/windows?url=' + urllib.request.quote('http://evil.test:8096'), expect=400)
            script = request(owner, '/drive/windows?url=' + urllib.request.quote(connect['addresses'][-1]['url']))
            assert b'net use' in script and bytes([13, 10]) in script
            local = connect['name'] + '.local:18096'
            request(owner, '/status', headers={'Host': local, 'Origin': 'http://' + local})
            request(owner, '/settings', 'PUT', {'name': 'Nome Invalido'}, expect=400)
            request(owner, '/settings', 'PUT', {'gpu': False}, headers={'Origin': 'https://attacker.test'}, expect=403)
            request(owner, '/status', headers={'Host': 'attacker.test'}, expect=403)
            request(owner, '/logout', 'POST')
            request(owner, '/media', expect=401)
            request(owner, '/login', 'POST', {'name': 'Teste local', 'password': 'sonam-test-only'})
            print('PASS: setup, auth, upload, range/seek, albums, genre policy, direct URL policy, PIN pairing, one-time token, progress, GPU preference, WebDAV drive + watched folder, CSRF, DNS rebinding, logout.')
            if args.serve:
                print(f'UI QA: {base} | user: Teste local | password: sonam-test-only', flush=True)
                while proc.poll() is None:
                    time.sleep(1)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill(); proc.wait()
