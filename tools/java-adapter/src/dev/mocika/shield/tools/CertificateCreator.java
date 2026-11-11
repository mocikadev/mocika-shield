package dev.mocika.shield.tools;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.channels.FileChannel;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.nio.file.LinkOption;
import java.nio.file.OpenOption;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.AccessDeniedException;
import java.security.Key;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.cert.Certificate;
import java.security.cert.X509Certificate;
import java.util.Arrays;
import java.util.Enumeration;
import java.util.HashSet;
import javax.security.auth.x500.X500Principal;

/** 只整理本次新建证书；密钥生成由 Rust 调用原 keytool，不操作历史材料。 */
final class CertificateCreator {
    static boolean supports(String[] request) {
        return (request.length == 7 && request[0].equals("keystore_create_prepare"))
            || (request.length == 8 && request[0].equals("keystore_create_finish"));
    }

    static int run(String version, String[] request) {
        char[] storePass = password("MOCIKA_SHIELD_TOOL_KS_PASS");
        char[] keyPass = password("MOCIKA_SHIELD_TOOL_KEY_PASS");
        try {
            String type = request[1];
            if (!type.equals("JKS") && !type.equals("PKCS12")) {
                return fail(version, request[0], "FORMAT_INVALID");
            }
            if (storePass.length < 6 || keyPass.length < 6) {
                return fail(version, request[0], "PASSWORD_INVALID");
            }
            if (type.equals("PKCS12") && (!ascii(storePass) || !Arrays.equals(storePass, keyPass))) {
                return fail(version, request[0], "PASSWORD_UNSUPPORTED");
            }
            if (request[2].isEmpty() || request[2].getBytes(StandardCharsets.UTF_8).length > 4096
                    || Long.parseLong(request[4]) <= 0
                    || (!request[5].equals("2048") && !request[5].equals("4096"))) {
                return fail(version, request[0], "INPUT_INVALID");
            }
            X500Principal subject;
            try {
                subject = new X500Principal(request[3]);
            } catch (IllegalArgumentException error) {
                return fail(version, request[0], "SUBJECT_INVALID");
            }
            Reply reply = new Reply(version, request[0]);
            if (request[0].equals("keystore_create_prepare")) {
                String dn = asciiSubject(subject);
                reply.add(",\"result\":{\"ascii_subject\":").quote(dn).add("}");
                KeyStore empty = KeyStore.getInstance(type);
                empty.load(null, storePass);
                ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                empty.store(buffer, storePass);
                writeReserved(Paths.get(request[6]), buffer.toByteArray());
            } else {
                finish(reply, request, storePass, keyPass, subject);
            }
            System.out.print(reply.finish());
            return 0;
        } catch (FileAlreadyExistsException error) {
            return fail(version, request[0], "OUTPUT_EXISTS");
        } catch (AccessDeniedException error) {
            return fail(version, request[0], "PERMISSION_DENIED");
        } catch (IllegalArgumentException error) {
            return fail(version, request[0], "INPUT_INVALID");
        } catch (Exception error) {
            return fail(version, request[0], "TOOL_FAILED");
        } finally {
            Arrays.fill(storePass, '\0');
            Arrays.fill(keyPass, '\0');
        }
    }

    private static String asciiSubject(X500Principal subject) {
        StringBuilder output = new StringBuilder();
        // 保留原有转义对；空格也转义，防止混合 UTF-8 转义后空格被标准解析器吞掉。
        byte[] name = subject.getName(X500Principal.RFC2253).getBytes(StandardCharsets.UTF_8);
        for (int i = 0; i < name.length; i++) {
            int ch = name[i] & 255;
            if (ch == '\\' && i + 1 < name.length) {
                int escaped = name[++i] & 255;
                output.append('\\');
                output.append(escaped == 32 ? "20" : String.valueOf((char) escaped));
            } else if (ch >= 33 && ch <= 126) {
                output.append((char) ch);
            } else {
                output.append('\\');
                output.append("0123456789ABCDEF".charAt(ch >>> 4));
                output.append("0123456789ABCDEF".charAt(ch & 15));
            }
        }
        String encoded = output.toString();
        if (!new X500Principal(encoded).equals(subject)) { throw new IllegalArgumentException(); }
        return encoded;
    }

    private static void finish(Reply reply, String[] request, char[] storePass, char[] keyPass,
            X500Principal subject) throws Exception {
        KeyStore source = KeyStore.getInstance(request[1]);
        try (InputStream input = Files.newInputStream(Paths.get(request[6]))) {
            source.load(input, storePass);
        }
        if (source.size() != 1 || !source.isKeyEntry("generated")) { throw new IllegalArgumentException(); }
        Key original = source.getKey("generated", keyPass);
        Certificate[] chain = source.getCertificateChain("generated");
        X509Certificate certificate = (X509Certificate) chain[0];
        if (!(original instanceof PrivateKey) || !certificate.getSubjectX500Principal().equals(subject)
                || !certificate.getIssuerX500Principal().equals(subject)
                || ((java.security.interfaces.RSAPublicKey) certificate.getPublicKey()).getModulus().bitLength()
                    != Integer.parseInt(request[5])) { throw new IllegalArgumentException(); }
        certificate.verify(certificate.getPublicKey());
        certificate.checkValidity();
        long duration = certificate.getNotAfter().getTime() - certificate.getNotBefore().getTime();
        if (duration != Math.multiplyExact(Long.parseLong(request[4]), 86400000L)) {
            throw new IllegalArgumentException();
        }
        KeyStore target = KeyStore.getInstance(request[1]);
        target.load(null, storePass);
        target.setKeyEntry(request[2], original, keyPass, chain);
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        target.store(buffer, storePass);
        byte[] encoded = buffer.toByteArray();
        try {
            if (encoded.length > 1048576) { throw new IllegalArgumentException(); }
            KeyStore check = KeyStore.getInstance(request[1]);
            check.load(new ByteArrayInputStream(encoded), storePass);
            Enumeration<String> aliases = check.aliases();
            String actual = aliases.nextElement();
            if (aliases.hasMoreElements() || !check.containsAlias(request[2])) { throw new IllegalArgumentException(); }
            Key recovered = check.getKey(actual, keyPass);
            if (!MessageDigest.isEqual(original.getEncoded(), recovered.getEncoded())) { throw new IllegalArgumentException(); }
            Certificate[] saved = check.getCertificateChain(actual);
            if (saved.length != chain.length) { throw new IllegalArgumentException(); }
            for (int i = 0; i < chain.length; i++) {
                if (!Arrays.equals(saved[i].getEncoded(), chain[i].getEncoded())) { throw new IllegalArgumentException(); }
            }
            Signature proof = Signature.getInstance("SHA256withRSA");
            proof.initSign((PrivateKey) recovered);
            proof.update(new byte[] {1, 4, 3});
            byte[] signature = proof.sign();
            proof.initVerify(saved[0]);
            proof.update(new byte[] {1, 4, 3});
            if (!proof.verify(signature)) { throw new IllegalArgumentException(); }
            StringBuilder hex = new StringBuilder();
            for (byte value : MessageDigest.getInstance("SHA-256").digest(saved[0].getEncoded())) {
                hex.append(String.format(java.util.Locale.ROOT, "%02X", value & 255));
            }
            // 完整结果先构造；验证失败或预算不足时不能留下可被当作成功的新证书。
            reply.add(",\"result\":{\"alias\":").quote(actual)
                .add(",\"fingerprint\":").quote(hex.toString()).add("}");
            writeReserved(Paths.get(request[7]), encoded);
        } finally {
            Arrays.fill(encoded, (byte) 0);
        }
    }

    private static void writeReserved(Path path, byte[] bytes) throws Exception {
        // 只能写 Rust 本次预留的普通空文件；不创建、不截断、不跟随链接。
        HashSet<OpenOption> options = new HashSet<OpenOption>();
        options.add(StandardOpenOption.WRITE);
        options.add(LinkOption.NOFOLLOW_LINKS);
        try (FileChannel channel = FileChannel.open(path, options)) {
            if (channel.size() != 0) { throw new FileAlreadyExistsException(""); }
            ByteBuffer data = ByteBuffer.wrap(bytes);
            while (data.hasRemaining()) { channel.write(data); }
            channel.force(true);
        }
    }

    private static char[] password(String name) {
        String value = System.getenv(name);
        return value == null ? new char[0] : value.toCharArray();
    }
    private static boolean ascii(char[] value) {
        for (char ch : value) { if (ch > 127) { return false; } }
        return true;
    }
    private static int fail(String version, String operation, String code) {
        System.out.print(new Reply(version, operation).add(",\"error\":").quote(code).finish());
        return 4;
    }
    private CertificateCreator() {}
}
