package com.voltnuerongrid.driver;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Minimal, dependency-free JSON parser sufficient for VoltNueronGrid HTTP
 * responses. Produces {@link Map}, {@link List}, {@link String}, {@link Double},
 * {@link Boolean}, or {@code null}.
 *
 * <p>Not a general-purpose JSON library — it intentionally has a tiny surface so
 * the driver carries no external dependencies. It handles objects, arrays,
 * strings (with the standard escapes), numbers, booleans, and null.
 */
final class Json {

    private final String s;
    private int pos;

    private Json(String s) {
        this.s = s;
    }

    /** Parses a JSON document into a Java object tree. */
    static Object parse(String text) {
        if (text == null) {
            return null;
        }
        Json p = new Json(text);
        p.skipWhitespace();
        Object v = p.readValue();
        p.skipWhitespace();
        return v;
    }

    private Object readValue() {
        skipWhitespace();
        if (pos >= s.length()) {
            throw new IllegalStateException("unexpected end of JSON");
        }
        char c = s.charAt(pos);
        switch (c) {
            case '{': return readObject();
            case '[': return readArray();
            case '"': return readString();
            case 't': case 'f': return readBoolean();
            case 'n': return readNull();
            default:  return readNumber();
        }
    }

    private Map<String, Object> readObject() {
        Map<String, Object> map = new LinkedHashMap<>();
        expect('{');
        skipWhitespace();
        if (peek() == '}') { pos++; return map; }
        while (true) {
            skipWhitespace();
            String key = readString();
            skipWhitespace();
            expect(':');
            Object value = readValue();
            map.put(key, value);
            skipWhitespace();
            char c = next();
            if (c == '}') break;
            if (c != ',') throw new IllegalStateException("expected ',' or '}' in object");
        }
        return map;
    }

    private List<Object> readArray() {
        List<Object> list = new ArrayList<>();
        expect('[');
        skipWhitespace();
        if (peek() == ']') { pos++; return list; }
        while (true) {
            list.add(readValue());
            skipWhitespace();
            char c = next();
            if (c == ']') break;
            if (c != ',') throw new IllegalStateException("expected ',' or ']' in array");
        }
        return list;
    }

    private String readString() {
        expect('"');
        StringBuilder sb = new StringBuilder();
        while (true) {
            char c = next();
            if (c == '"') break;
            if (c == '\\') {
                char e = next();
                switch (e) {
                    case '"':  sb.append('"');  break;
                    case '\\': sb.append('\\'); break;
                    case '/':  sb.append('/');  break;
                    case 'b':  sb.append('\b'); break;
                    case 'f':  sb.append('\f'); break;
                    case 'n':  sb.append('\n'); break;
                    case 'r':  sb.append('\r'); break;
                    case 't':  sb.append('\t'); break;
                    case 'u':
                        String hex = s.substring(pos, pos + 4);
                        pos += 4;
                        sb.append((char) Integer.parseInt(hex, 16));
                        break;
                    default: throw new IllegalStateException("invalid escape \\" + e);
                }
            } else {
                sb.append(c);
            }
        }
        return sb.toString();
    }

    private Object readNumber() {
        int start = pos;
        while (pos < s.length()) {
            char c = s.charAt(pos);
            if ((c >= '0' && c <= '9') || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E') {
                pos++;
            } else {
                break;
            }
        }
        return Double.parseDouble(s.substring(start, pos));
    }

    private Boolean readBoolean() {
        if (s.startsWith("true", pos)) { pos += 4; return Boolean.TRUE; }
        if (s.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
        throw new IllegalStateException("invalid boolean literal");
    }

    private Object readNull() {
        if (s.startsWith("null", pos)) { pos += 4; return null; }
        throw new IllegalStateException("invalid null literal");
    }

    private void skipWhitespace() {
        while (pos < s.length() && Character.isWhitespace(s.charAt(pos))) {
            pos++;
        }
    }

    private char peek() {
        skipWhitespace();
        return pos < s.length() ? s.charAt(pos) : '\0';
    }

    private char next() {
        if (pos >= s.length()) {
            throw new IllegalStateException("unexpected end of JSON");
        }
        return s.charAt(pos++);
    }

    private void expect(char c) {
        char actual = next();
        if (actual != c) {
            throw new IllegalStateException("expected '" + c + "' but found '" + actual + "'");
        }
    }
}
