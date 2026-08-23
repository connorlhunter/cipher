import { useForm } from "@tanstack/react-form";
import { useMutation } from "@tanstack/react-query";
import { LoaderCircle, LockKeyhole } from "lucide-react";
import { type JSX, useEffect, useId, useState } from "react";
import { z } from "zod";

import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card } from "../../components/ui/card";
import { Input } from "../../components/ui/input";
import { Label } from "../../components/ui/label";
import {
  desktopAuthenticate,
  type DesktopAuthenticationRequest,
  type DesktopAuthenticationView,
} from "../../desktop";

const signInValues = z.object({
  identifier: z.string().trim().min(1, "Enter your email or username.").max(320),
  password: z.string().min(1, "Enter your password.").max(512),
});

type SignInValues = z.infer<typeof signInValues>;

const initialValues: SignInValues = { identifier: "", password: "" };

const verificationValues = z.object({
  code: z.string().regex(/^\d{6}$/, "Enter the six-digit verification code."),
});

const passwordResetRequestValues = z.object({
  identifier: z.string().trim().min(1, "Enter your email or username.").max(320),
});

const passwordResetConfirmationValues = passwordResetRequestValues.extend({
  code: z.string().regex(/^\d{6}$/, "Enter the six-digit recovery code."),
  newPassword: z.string().min(12, "Use at least 12 characters.").max(512),
});

type IdentifierStatus = "idle" | "checking" | "ready" | "invalid";

/** A one-time form that submits credentials directly to the native authentication boundary. */
export function SignInForm({
  authenticate = desktopAuthenticate,
}: {
  authenticate?: (request: DesktopAuthenticationRequest) => Promise<DesktopAuthenticationView>;
} = {}): JSX.Element {
  const identifierId = useId();
  const passwordId = useId();
  const verificationId = useId();
  const [result, setResult] = useState<DesktopAuthenticationView | undefined>();
  const [challengeRequired, setChallengeRequired] = useState(false);
  const [identifier, setIdentifier] = useState("");
  const [identifierStatus, setIdentifierStatus] = useState<IdentifierStatus>("idle");
  const mutation = useMutation({
    mutationFn: (request: DesktopAuthenticationRequest) => authenticate(request),
    retry: false,
  });
  const form = useForm({
    defaultValues: initialValues,
    validators: { onSubmit: signInValues },
    onSubmit: async ({ value }) => {
      const request: DesktopAuthenticationRequest = {
        flow: "sign_in",
        identifier: value.identifier.trim(),
        password: value.password,
      };
      try {
        const response = await mutation.mutateAsync(request);
        setChallengeRequired(response.state === "challenge_required");
        setResult(response);
      } catch {
        setResult({
          state: "failed",
          message: "Cipher couldn't complete sign-in. Check your connection and try again.",
        });
      } finally {
        form.reset(initialValues);
        setIdentifier("");
      }
    },
  });

  useEffect(() => {
    const value = identifier.trim();
    if (!value) {
      setIdentifierStatus("idle");
      return;
    }
    setIdentifierStatus("checking");
    const timer = window.setTimeout(() => {
      setIdentifierStatus(isValidIdentifier(value) ? "ready" : "invalid");
    }, 350);
    return () => window.clearTimeout(timer);
  }, [identifier]);

  return (
    <Card className="relative w-full max-w-md p-panel">
      <div className="absolute right-5 top-5 flex size-9 items-center justify-center rounded-md bg-elevated p-1.5">
        <img alt="Cipher" className="size-full" src="/cipher-mark.svg" />
      </div>
      <Badge
        aria-label="End-to-end encryption"
        title="End-to-end encrypted: only you and the people you message can read your conversations. Your account uses an encryption key generated and securely stored on your device to encrypt your data. The key never leaves your device and is deleted when you uninstall the app. Your data is also deleted when you delete your account. Not even Cipher staff can access or read your encrypted conversations."
        tone="neutral"
      >
        <LockKeyhole aria-hidden="true" className="mr-1" size={13} strokeWidth={1.8} />
        E2EE
      </Badge>
      <h1 className="type-page-title mt-paragraph">Welcome to Cipher</h1>
      <p className="type-body-muted mt-paragraph">Sign in to continue.</p>
      {challengeRequired ? (
        <VerificationForm
          id={verificationId}
          onResult={(response) => {
            setChallengeRequired(response.state !== "authenticated");
            setResult(response);
          }}
          submit={(request) => mutation.mutateAsync(request)}
        />
      ) : (
        <form
          className="mt-section grid gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            void form.handleSubmit();
          }}
        >
          <form.Field name="identifier">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={identifierId}>Email or username</Label>
                <Input
                  aria-describedby={
                    field.state.meta.errors.length ? `${identifierId}-error` : undefined
                  }
                  autoComplete="username"
                  id={identifierId}
                  maxLength={320}
                  onBlur={field.handleBlur}
                  onChange={(event) => {
                    field.handleChange(event.target.value);
                    setIdentifier(event.target.value);
                  }}
                  value={field.state.value}
                />
                <IdentifierStatus status={identifierStatus} />
                <FieldError id={`${identifierId}-error`} message={field.state.meta.errors[0]} />
              </div>
            )}
          </form.Field>
          <form.Field name="password">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={passwordId}>Password</Label>
                <Input
                  aria-describedby={
                    field.state.meta.errors.length ? `${passwordId}-error` : undefined
                  }
                  autoComplete="current-password"
                  id={passwordId}
                  maxLength={512}
                  disabled={identifierStatus !== "ready"}
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  type="password"
                  value={field.state.value}
                />
                <FieldError id={`${passwordId}-error`} message={field.state.meta.errors[0]} />
              </div>
            )}
          </form.Field>
          <form.Subscribe
            selector={(state) =>
              [state.canSubmit, state.isSubmitting, state.values.password] as const
            }
          >
            {([canSubmit, isSubmitting, password]) => (
              <Button
                disabled={!canSubmit || isSubmitting || !password || identifierStatus !== "ready"}
                type="submit"
              >
                {isSubmitting ? <LoadingLabel label="Signing in…" /> : "Sign in"}
              </Button>
            )}
          </form.Subscribe>
        </form>
      )}
      {result ? (
        <p
          aria-live="polite"
          className={
            result.state === "authenticated"
              ? "mt-paragraph type-caption text-success"
              : "mt-paragraph type-caption text-destructive"
          }
          role={result.state === "failed" ? "alert" : "status"}
        >
          {result.message}
        </p>
      ) : null}
    </Card>
  );
}

function VerificationForm({
  id,
  onResult,
  submit,
}: {
  id: string;
  onResult: (response: DesktopAuthenticationView) => void;
  submit: (request: DesktopAuthenticationRequest) => Promise<DesktopAuthenticationView>;
}): JSX.Element {
  const form = useForm({
    defaultValues: { code: "" },
    validators: { onSubmit: verificationValues },
    onSubmit: async ({ value }) => {
      try {
        onResult(await submit({ flow: "continue_challenge", code: value.code.trim() }));
      } catch {
        onResult({
          state: "failed",
          message: "Cipher couldn't verify that code. Check your connection and try again.",
        });
      } finally {
        form.reset({ code: "" });
      }
    },
  });
  return (
    <form
      className="mt-section grid gap-4"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        void form.handleSubmit();
      }}
    >
      <form.Field name="code">
        {(field) => (
          <div className="grid gap-2">
            <Label htmlFor={id}>Verification code</Label>
            <Input
              aria-describedby={field.state.meta.errors.length ? `${id}-error` : undefined}
              autoComplete="one-time-code"
              id={id}
              inputMode="numeric"
              maxLength={6}
              onBlur={field.handleBlur}
              onChange={(event) => field.handleChange(event.target.value)}
              value={field.state.value}
            />
            <FieldError id={`${id}-error`} message={field.state.meta.errors[0]} />
          </div>
        )}
      </form.Field>
      <form.Subscribe selector={(state) => [state.canSubmit, state.isSubmitting] as const}>
        {([canSubmit, isSubmitting]) => (
          <Button disabled={!canSubmit || isSubmitting} type="submit">
            {isSubmitting ? <LoadingLabel label="Verifying…" /> : "Verify"}
          </Button>
        )}
      </form.Subscribe>
    </form>
  );
}

/** Password recovery is an explicit route so sign-in remains focused and credentials stay transient. */
export function PasswordResetForm({
  authenticate = desktopAuthenticate,
}: {
  authenticate?: (request: DesktopAuthenticationRequest) => Promise<DesktopAuthenticationView>;
} = {}): JSX.Element {
  const requestIdentifierId = useId();
  const confirmIdentifierId = useId();
  const codeId = useId();
  const passwordId = useId();
  const [awaitingCode, setAwaitingCode] = useState(false);
  const [result, setResult] = useState<DesktopAuthenticationView | undefined>();
  const mutation = useMutation({
    mutationFn: (request: DesktopAuthenticationRequest) => authenticate(request),
    retry: false,
  });
  const requestForm = useForm({
    defaultValues: { identifier: "" },
    validators: { onSubmit: passwordResetRequestValues },
    onSubmit: async ({ value }) => {
      try {
        const response = await mutation.mutateAsync({
          flow: "begin_password_reset",
          identifier: value.identifier.trim(),
        });
        setAwaitingCode(response.state === "password_reset_required");
        setResult(response);
      } catch {
        setResult({ state: "failed", message: "Password reset is unavailable right now." });
      } finally {
        requestForm.reset({ identifier: "" });
      }
    },
  });
  const confirmationForm = useForm({
    defaultValues: { identifier: "", code: "", newPassword: "" },
    validators: { onSubmit: passwordResetConfirmationValues },
    onSubmit: async ({ value }) => {
      try {
        setResult(
          await mutation.mutateAsync({
            flow: "confirm_password_reset",
            identifier: value.identifier.trim(),
            code: value.code.trim(),
            newPassword: value.newPassword,
          }),
        );
      } catch {
        setResult({ state: "failed", message: "Password reset is unavailable right now." });
      } finally {
        confirmationForm.reset({ identifier: "", code: "", newPassword: "" });
      }
    },
  });

  return (
    <Card className="w-full max-w-md p-panel">
      <Badge tone="neutral">Account recovery</Badge>
      <h1 className="type-page-title mt-paragraph">Reset your password</h1>
      <p className="type-body-muted mt-paragraph">
        {awaitingCode
          ? "Enter the code from your email and choose a new password."
          : "We’ll send a recovery code if your account is eligible."}
      </p>
      {awaitingCode ? (
        <form
          className="mt-section grid gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            void confirmationForm.handleSubmit();
          }}
        >
          <confirmationForm.Field name="identifier">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={confirmIdentifierId}>Email or username</Label>
                <Input
                  autoComplete="username"
                  id={confirmIdentifierId}
                  maxLength={320}
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  value={field.state.value}
                />
                <FieldError
                  id={`${confirmIdentifierId}-error`}
                  message={field.state.meta.errors[0]}
                />
              </div>
            )}
          </confirmationForm.Field>
          <confirmationForm.Field name="code">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={codeId}>Recovery code</Label>
                <Input
                  autoComplete="one-time-code"
                  id={codeId}
                  inputMode="numeric"
                  maxLength={6}
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  value={field.state.value}
                />
                <FieldError id={`${codeId}-error`} message={field.state.meta.errors[0]} />
              </div>
            )}
          </confirmationForm.Field>
          <confirmationForm.Field name="newPassword">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={passwordId}>New password</Label>
                <Input
                  autoComplete="new-password"
                  id={passwordId}
                  maxLength={512}
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  type="password"
                  value={field.state.value}
                />
                <FieldError id={`${passwordId}-error`} message={field.state.meta.errors[0]} />
              </div>
            )}
          </confirmationForm.Field>
          <confirmationForm.Subscribe
            selector={(state) => [state.canSubmit, state.isSubmitting] as const}
          >
            {([canSubmit, isSubmitting]) => (
              <Button disabled={!canSubmit || isSubmitting} type="submit">
                {isSubmitting ? <LoadingLabel label="Updating password…" /> : "Update password"}
              </Button>
            )}
          </confirmationForm.Subscribe>
        </form>
      ) : (
        <form
          className="mt-section grid gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            void requestForm.handleSubmit();
          }}
        >
          <requestForm.Field name="identifier">
            {(field) => (
              <div className="grid gap-2">
                <Label htmlFor={requestIdentifierId}>Email or username</Label>
                <Input
                  aria-describedby={
                    field.state.meta.errors.length ? `${requestIdentifierId}-error` : undefined
                  }
                  autoComplete="username"
                  id={requestIdentifierId}
                  maxLength={320}
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  value={field.state.value}
                />
                <FieldError
                  id={`${requestIdentifierId}-error`}
                  message={field.state.meta.errors[0]}
                />
              </div>
            )}
          </requestForm.Field>
          <requestForm.Subscribe
            selector={(state) => [state.canSubmit, state.isSubmitting] as const}
          >
            {([canSubmit, isSubmitting]) => (
              <Button disabled={!canSubmit || isSubmitting} type="submit">
                {isSubmitting ? <LoadingLabel label="Sending code…" /> : "Send recovery code"}
              </Button>
            )}
          </requestForm.Subscribe>
        </form>
      )}
      {result ? (
        <p
          aria-live="polite"
          className={
            result.state === "failed"
              ? "mt-paragraph type-caption text-destructive"
              : "mt-paragraph type-caption text-success"
          }
          role={result.state === "failed" ? "alert" : "status"}
        >
          {result.message}
        </p>
      ) : null}
    </Card>
  );
}

function FieldError({ id, message }: { id: string; message: unknown }): JSX.Element | null {
  if (typeof message !== "string" || message.length === 0) {
    return null;
  }
  return (
    <p className="type-caption text-destructive" id={id} role="alert">
      {message}
    </p>
  );
}

function IdentifierStatus({ status }: { status: IdentifierStatus }): JSX.Element | null {
  if (status === "idle") {
    return null;
  }
  if (status === "checking") {
    return (
      <p className="type-caption flex items-center gap-2 text-muted" role="status">
        <LoaderCircle aria-hidden="true" className="animate-spin" size={14} strokeWidth={1.8} />
        Checking…
      </p>
    );
  }
  return (
    <p
      className={status === "ready" ? "type-caption text-success" : "type-caption text-destructive"}
    >
      {status === "ready" ? "Looks good." : "Enter a valid email or username."}
    </p>
  );
}

function LoadingLabel({ label }: { label: string }): JSX.Element {
  return (
    <>
      <LoaderCircle aria-hidden="true" className="animate-spin" size={16} strokeWidth={1.8} />
      {label}
    </>
  );
}

function isValidIdentifier(value: string): boolean {
  const email = value.split("@");
  const validEmail = email.length === 2 && email[0]!.length > 0 && email[1]!.includes(".");
  const validUsername = /^[A-Za-z0-9._-]{3,64}$/u.test(value);
  return validEmail || validUsername;
}
