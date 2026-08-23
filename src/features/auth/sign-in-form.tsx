import { useForm } from "@tanstack/react-form";
import { useMutation } from "@tanstack/react-query";
import { type JSX, useId, useState } from "react";
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

/** A one-time form that submits credentials directly to the native authentication boundary. */
export function SignInForm(): JSX.Element {
  const identifierId = useId();
  const passwordId = useId();
  const verificationId = useId();
  const [result, setResult] = useState<DesktopAuthenticationView | undefined>();
  const [challengeRequired, setChallengeRequired] = useState(false);
  const mutation = useMutation({
    mutationFn: (request: DesktopAuthenticationRequest) => desktopAuthenticate(request),
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
        setResult({ state: "failed", message: "Authentication is temporarily unavailable." });
      } finally {
        form.reset(initialValues);
      }
    },
  });

  return (
    <Card className="w-full max-w-md p-panel">
      <Badge tone="neutral">Secure sign-in</Badge>
      <h1 className="type-page-title mt-paragraph">Welcome to Cipher</h1>
      <p className="type-body-muted mt-paragraph">
        Your credentials are submitted once to the native desktop security boundary.
      </p>
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
                  onChange={(event) => field.handleChange(event.target.value)}
                  value={field.state.value}
                />
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
                  onBlur={field.handleBlur}
                  onChange={(event) => field.handleChange(event.target.value)}
                  type="password"
                  value={field.state.value}
                />
                <FieldError id={`${passwordId}-error`} message={field.state.meta.errors[0]} />
              </div>
            )}
          </form.Field>
          <form.Subscribe selector={(state) => [state.canSubmit, state.isSubmitting] as const}>
            {([canSubmit, isSubmitting]) => (
              <Button disabled={!canSubmit || isSubmitting} type="submit">
                {isSubmitting ? "Signing in…" : "Sign in"}
              </Button>
            )}
          </form.Subscribe>
        </form>
      )}
      {result ? (
        <p
          aria-live="polite"
          className={
            result.state === "authenticated" ? "mt-paragraph type-caption text-success" : "mt-paragraph type-caption text-muted"
          }
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
        onResult(
          await submit({ flow: "continue_challenge", code: value.code.trim() }),
        );
      } catch {
        onResult({ state: "failed", message: "Authentication is temporarily unavailable." });
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
            {isSubmitting ? "Verifying…" : "Verify"}
          </Button>
        )}
      </form.Subscribe>
    </form>
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
