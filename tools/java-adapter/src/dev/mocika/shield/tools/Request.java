package dev.mocika.shield.tools;

import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;

/** 有限私有请求解码；不解释证书、路径或工具操作。 */
final class Request {
    static final int PROTOCOL = 4;
    static final int MAGIC = 0x4d534a41;
    static final int MAX_BYTES = 65536;

    static String[] read(InputStream source) throws IOException {
        DataInputStream input = new DataInputStream(source);
        if (input.readInt() != MAGIC || input.readInt() != PROTOCOL) {
            throw new IOException();
        }
        int count = input.readInt();
        if (count < 1 || count > 64) {
            throw new IOException();
        }
        int remaining = MAX_BYTES - 12;
        String[] values = new String[count];
        for (int i = 0; i < count; i++) {
            int length = input.readInt();
            remaining -= 4;
            if (length < 0 || length > remaining) {
                throw new IOException();
            }
            byte[] bytes = new byte[length];
            input.readFully(bytes);
            values[i] = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes)).toString();
            remaining -= length;
        }
        if (input.read() != -1) {
            throw new IOException();
        }
        return values;
    }

    private Request() {}
}
