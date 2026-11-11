package dev.mocika.shield.tools;

/** 有界完整 JSON 结果；不流式回传部分 Alias，不打印异常或输入材料。 */
final class Reply {
    static final int MAX_BYTES = 65536;
    private final StringBuilder output = new StringBuilder();

    Reply(String version, String operation) {
        add("{\"protocol_version\":" + Request.PROTOCOL + ",\"application_version\":");
        quote(version);
        add(",\"operation\":");
        quote(operation);
    }

    Reply add(String value) {
        if (output.length() + value.length() > MAX_BYTES) {
            throw new IllegalArgumentException();
        }
        output.append(value);
        return this;
    }

    Reply quote(String value) {
        add("\"");
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if (ch == '\\' || ch == '"') {
                add("\\" + ch);
            } else if (ch >= 32 && ch <= 126) {
                add(String.valueOf(ch));
            } else {
                // 全部非 ASCII 以 UTF-16 JSON 转义输出，代理项对保持原字符串不变。
                String hex = Integer.toHexString(ch);
                add("\\u" + "0000".substring(hex.length()) + hex);
            }
        }
        return add("\"");
    }

    String finish() {
        return add("}").output.toString();
    }
}
