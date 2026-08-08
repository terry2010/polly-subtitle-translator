// 登录页（FP-P0-6）
// 等待状态 / 错误提示 / 重试按钮
import { useTranslation } from "react-i18next";
import { Loader2, LogIn, AlertCircle, WifiOff, RefreshCw } from "lucide-react";
import { Button } from "../components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { useAuthStore } from "../stores/authStore";

export function LoginView() {
  const { t } = useTranslation();
  const { status, error, login, clearError } = useAuthStore();

  const isLoggingIn = status === "logging_in";

  return (
    <div className="flex items-center justify-center min-h-screen bg-background p-4">
      <Card className="w-full max-w-md">
        <CardHeader className="text-center">
          <CardTitle className="text-xl">{t("auth.loginTitle")}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col items-center gap-4">
          {isLoggingIn ? (
            <>
              <Loader2 className="h-12 w-12 animate-spin text-primary" />
              <p className="text-sm text-muted-foreground text-center">
                {t("auth.waitingForLogin")}
              </p>
              <p className="text-xs text-muted-foreground text-center">
                {t("auth.browserOpenedHint")}
              </p>
            </>
          ) : status === "offline" ? (
            <>
              <WifiOff className="h-12 w-12 text-muted-foreground" />
              <p className="text-sm text-muted-foreground text-center">
                {t("auth.offlineMode")}
              </p>
              {error && (
                <p className="text-xs text-destructive text-center">{error}</p>
              )}
              <Button onClick={() => login()} className="w-full">
                <LogIn className="h-4 w-4 mr-2" />
                {t("auth.retryLogin")}
              </Button>
            </>
          ) : status === "error" ? (
            <>
              <AlertCircle className="h-12 w-12 text-destructive" />
              <p className="text-sm text-destructive text-center">
                {error ?? t("auth.loginFailed")}
              </p>
              <Button onClick={() => { clearError(); login(); }} className="w-full">
                <RefreshCw className="h-4 w-4 mr-2" />
                {t("auth.retry")}
              </Button>
            </>
          ) : (
            <>
              <LogIn className="h-12 w-12 text-primary" />
              <p className="text-sm text-muted-foreground text-center">
                {t("auth.loginPrompt")}
              </p>
              <Button
                onClick={() => login()}
                disabled={isLoggingIn}
                className="w-full"
              >
                <LogIn className="h-4 w-4 mr-2" />
                {t("auth.startLogin")}
              </Button>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
