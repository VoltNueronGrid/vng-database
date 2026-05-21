package com.voltnuerongrid.driver;

/**
 * Represents an HTTP response received after executing a {@link DriverRequest}.
 *
 * <p>Returned by {@link VoltNueronGridDriver#execute(DriverRequest)}.
 */
public final class DriverResponse {

    /** HTTP status code, e.g. 200 or 503. */
    private final int statusCode;

    /** Response body as a UTF-8 string (typically JSON). */
    private final String body;

    /**
     * Constructs a driver response.
     *
     * @param statusCode HTTP status code
     * @param body       response body text (never {@code null}; use empty string for no body)
     */
    public DriverResponse(int statusCode, String body) {
        this.statusCode = statusCode;
        this.body = body != null ? body : "";
    }

    /** Returns the HTTP status code. */
    public int statusCode() {
        return statusCode;
    }

    /** Returns the response body as a string (never {@code null}). */
    public String body() {
        return body;
    }

    /** Returns {@code true} if the status code is in the 2xx range. */
    public boolean isSuccess() {
        return statusCode >= 200 && statusCode < 300;
    }

    @Override
    public String toString() {
        return "DriverResponse{statusCode=" + statusCode +
               ", body=" + (body.length() > 80 ? "'" + body.substring(0, 80) + "...'" : "'" + body + "'") +
               "}";
    }
}
