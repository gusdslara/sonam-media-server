"""Cross-compile the Sonam server (JNI bridge) for Android and copy the
shared libraries into the Gradle project's jniLibs.

Usage: python scripts/android-rust.py [--abi arm64-v8a --abi armeabi-v7a ...] [--debug]
Requires: Rust targets (rustup target add aarch64-linux-android armv7-linux-androideabi
x86_64-linux-android) and an Android NDK (ANDROID_NDK_HOME or <sdk>/ndk/<version>).
"""
import argparse
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / 'clients/android/rust'
JNI = ROOT / 'clients/android/app/src/main/jniLibs'
API = 24  # getifaddrs (network interfaces) needs Android 7.0+
ABIS = {
    'arm64-v8a': ('aarch64-linux-android', 'aarch64-linux-android'),
    'armeabi-v7a': ('armv7-linux-androideabi', 'armv7a-linux-androideabi'),
    'x86_64': ('x86_64-linux-android', 'x86_64-linux-android'),
}


def find_ndk() -> Path:
    for var in ('ANDROID_NDK_HOME', 'ANDROID_NDK_ROOT', 'NDK_HOME'):
        if os.environ.get(var):
            return Path(os.environ[var])
    sdk = os.environ.get('ANDROID_HOME') or os.environ.get('ANDROID_SDK_ROOT')
    candidates = [Path(sdk)] if sdk else []
    if os.name == 'nt':
        candidates.append(Path(os.environ.get('LOCALAPPDATA', '')) / 'Android/Sdk')
    elif sys.platform == 'darwin':
        candidates.append(Path.home() / 'Library/Android/sdk')
    else:
        candidates.append(Path.home() / 'Android/Sdk')
    for c in candidates:
        versions = sorted((c / 'ndk').glob('*'), key=lambda p: [int(x) for x in p.name.split('.') if x.isdigit()])
        if versions:
            return versions[-1]
    sys.exit('Android NDK não encontrado. Defina ANDROID_NDK_HOME.')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--abi', action='append', choices=ABIS, help='padrão: arm64-v8a e armeabi-v7a')
    parser.add_argument('--debug', action='store_true')
    args = parser.parse_args()
    abis = args.abi or ['arm64-v8a', 'armeabi-v7a']
    ndk = find_ndk()
    host = {'Windows': 'windows-x86_64', 'Darwin': 'darwin-x86_64'}.get(platform.system(), 'linux-x86_64')
    bin_dir = ndk / 'toolchains/llvm/prebuilt' / host / 'bin'
    ext = '.cmd' if os.name == 'nt' else ''
    profile = 'debug' if args.debug else 'release'
    for abi in abis:
        target, clang_prefix = ABIS[abi]
        clang = bin_dir / f'{clang_prefix}{API}-clang{ext}'
        if not clang.exists():
            sys.exit(f'Compilador não encontrado: {clang}')
        key = target.replace('-', '_')
        env = {
            **os.environ,
            f'CARGO_TARGET_{key.upper()}_LINKER': str(clang),
            f'CC_{key}': str(clang),
            f'AR_{key}': str(bin_dir / f'llvm-ar{".exe" if os.name == "nt" else ""}'),
        }
        cmd = ['cargo', 'build', '--locked' if (CRATE / 'Cargo.lock').exists() else '--quiet',
               '--manifest-path', str(CRATE / 'Cargo.toml'), '--target', target, '-j', os.environ.get('SONAM_JOBS', '4')]
        if not args.debug:
            cmd.append('--release')
        print(f'> {abi}: {" ".join(cmd)}', flush=True)
        subprocess.run(cmd, env=env, check=True)
        out = JNI / abi
        out.mkdir(parents=True, exist_ok=True)
        shutil.copy2(CRATE / 'target' / target / profile / 'libsonam_android.so', out / 'libsonam_android.so')
        print(f'  -> {out / "libsonam_android.so"}', flush=True)


if __name__ == '__main__':
    main()
