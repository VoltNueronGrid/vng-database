package com.voltnuerongrid.driver;

/**
 * Typed error thrown by the VoltNueronGrid Java driver.
 * Mirrors the driver-core-contract error envelope.
 */
public class DriverError extends RuntimeException {

    /**
     * Classifies the root cause of a driver failure.
     */
    public enum Kind {
        /** Configuration or parameter validation failed. */
        VALIDATION,
        /** Network-level failure (connection refused, DNS, etc.). */
        TRANSPORT,
        /** Server returned a non-successful HTTP status. */
        HTTP_STATUS,
        /** Request exceeded the configured timeout. */
        TIMEOUT,
        /** Request was cancelled by the caller. */
        CANCELLED
    }

    private final Kind kind;
    private final int statusCode;

    /**
     * @param kind       error category
     * @param message    human-readable description
     * @param statusCode HTTP status code (0 if not applicable)
     */
    public DriverError(Kind kind, String message, int statusCode) {
        super(message);
        this.kind = kind;
        this.statusCode = statusCode;
    }

    public DriverError(Kind kind, String message, int statusCode, Throwable cause) {
        super(message, cause);
        this.kind = kind;
        this.statusCode = statusCode;
    }

    /** Returns the error category. */
    public Kind getKind() {
        return kind;
    }

    /**
     * Returns the HTTP status code for {@link Kind#HTTP_STATUS} errors, or 0 otherwise.
     */
    public int getStatusCode() {
        return statusCode;
    }

    // --- Factory helpers ---

    public static DriverError validation(String message) {
        return new DriverError(Kind.VALIDATION, message, 0);
    }

    public static DriverError transport(String message) {
        return new DriverError(Kind.TRANSPORT, message, 0);
    }

    public static DriverError transport(String message, Throwable cause) {
        return new DriverError(Kind.TRANSPORT, message, 0, cause);
    }

    public static DriverError httpStatus(int statusCode, String message) {
        return new DriverError(Kind.HTTP_STATUS, message, statusCode);
    }

    public static DriverError timeout(String message) {
        return new DriverError(Kind.TIMEOUT, message, 0);
    }

    public static DriverError cancelled(String message) {
        return new DriverError(Kind.CANCELLED, message, 0);
    }

    @Override
    public String toString() {
        return "DriverError{kind=" + kind + ", statusCode=" + statusCode + ", message='" + getMessage() + "'}";
    }
}
