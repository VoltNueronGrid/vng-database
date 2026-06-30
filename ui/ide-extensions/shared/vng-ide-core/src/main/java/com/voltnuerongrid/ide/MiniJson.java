package com.voltnuerongrid.ide;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * D-5: minimal, dependency-free JSON parser for IDE extensions. Supports the
 * subset needed to read a {@code /api/v1/sql/execute} response: objects, arrays,
 * strings, numbers, booleans, and null. Not a general-purpose parser — it is
 * deliberately tiny so the IDE cores carry no third-party JSON dependency.
 */
final class MiniJson {

    private final String src;
    private int pos;

    private MiniJson(String src) {
        this.src = src;
    }

    /** Parse a JSON document into Map/List/String/Double/Boolean/null. */
    static Object parse(String json) {
        MiniJson p = new MiniJson(json == null ? "" : json);
        p.skipWs();
        if (p.pos >= p.src.length()) {
            return null;
        }
        return p.value();
    }

    private Object value() {
        skipWs();
        char c = peek();
        switch (c) {
            case '{':
                return object();
            case '[':
                return array();
            case '"':
                return string();
            case 't':
            case 'f':
                return bool();
            case 'n':
                literalNull();
                return null;
            default:
                return number();
        }
    }

    private Map<String, Object> object() {
        Map<String, Object> map = new LinkedHashMap<>();
        expect('{');
        skipWs();
        if (peek() == '}') {
            pos++;
            return map;
        }
        while (true) {
            skipWs();
            String key = string();
            skipWs();
            expect(':');
            Object v = value();
            map.put(key, v);
            skipWs();
            char c = next();
            if (c == ',') {
                continue;
            }
            if (c == '}') {
                break;
            }
            throw new IllegalStateException("expected , or } at " + pos);
        }
        return map;
    }

    private List<Object> array() {
        List<Object> list = new ArrayList<>();
        expect('[');
        skipWs();
        if (peek() == ']') {
            pos++;
            return list;
        }
        while (true) {
            list.add(value());
            skipWs();
            char c = next();
            if (c == ',') {
                continue;
            }
            if (c == ']') {
                break;
            }
            throw new IllegalStateException("expected , or ] at " + pos);
        }
        return list;
    }

    private String string() {
        expect('"');
        StringBuilder sb = new StringBuilder();
        while (true) {
            char c = next();
            if (c == '"') {
                break;
            }
            if (c == '\\') {
                char e = next();
                switch (e) {
                    case '"': sb.append('"'); break;
                    case '\\': sb.append('\\'); break;
                    case '/': sb.append('/'); break;
                    case 'b': sb.append('\b'); break;
                    case 'f': sb.append('\f'); break;
                    case 'n': sb.append('\n'); break;
                    case 'r': sb.append('\r'); break;
                    case 't': sb.append('\t'); break;
                    case 'u':
                        String hex = src.substring(pos, pos + 4);
                        pos += 4;
                        sb.append((char) Integer.parseInt(hex, 16));
                        break;
                    default: sb.append(e);
                }
            } else {
                sb.append(c);
            }
        }
        return sb.toString();
    }

    private Object number() {
        int start = pos;
        while (pos < src.length() && "+-0123456789.eE".indexOf(src.charAt(pos)) >= 0) {
            pos++;
        }
        return Double.parseDouble(src.substring(start, pos));
    }

    private Boolean bool() {
        if (src.startsWith("true", pos)) {
            pos += 4;
            return Boolean.TRUE;
        }
        pos += 5; // false
        return Boolean.FALSE;
    }

    private void literalNull() {
        pos += 4;
    }

    private void skipWs() {
        while (pos < src.length() && Character.isWhitespace(src.charAt(pos))) {
            pos++;
        }
    }

    private char peek() {
        return src.charAt(pos);
    }

    private char next() {
        return src.charAt(pos++);
    }

    private void expect(char c) {
        if (next() != c) {
            throw new IllegalStateException("expected '" + c + "' at " + (pos - 1));
        }
    }
}
