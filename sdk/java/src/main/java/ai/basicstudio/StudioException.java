package ai.basicstudio;

/** 网关返回错误时抛出的异常。 */
public class StudioException extends RuntimeException {

    private final int status;
    private final String body;

    public StudioException(String message, int status, String body) {
        super(message);
        this.status = status;
        this.body = body;
    }

    public StudioException(String message) {
        this(message, 0, null);
    }

    public int status() {
        return status;
    }

    public String body() {
        return body;
    }

    /** 是否值得重试：网络异常、429、5xx。 */
    public boolean retryable() {
        return status == 0 || status == 429 || status >= 500;
    }
}