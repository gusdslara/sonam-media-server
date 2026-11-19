package org.sonam.server;

/** Native Sonam server (Rust, same code as the desktop server). */
final class ServerBridge {
    static { System.loadLibrary("sonam_android"); }
    private ServerBridge() {}

    /** Starts the server on 0.0.0.0:port. Returns null on success or an error message. */
    static native String start(String dataDir, int port);
    static native void stop();
    static native boolean isRunning();
}
