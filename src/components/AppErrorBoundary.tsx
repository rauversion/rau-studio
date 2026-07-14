import { AlertTriangle, Copy, RefreshCcw, RotateCcw } from "lucide-react";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "./ui/button";

type AppErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorSource = "render" | "window" | "promise";

type ErrorIncident = {
  message: string;
  details: string;
  source: ErrorSource;
  occurredAt: string;
  location: string;
};

type AppErrorBoundaryState = {
  incident: ErrorIncident | null;
  copied: boolean;
};

const SECRET_PATTERNS: Array<[RegExp, string]> = [
  [/(\b(?:api[_-]?key|access[_-]?token|auth[_-]?token|token|key)\s*[:=]\s*)[^\s,;]+/gi, "$1[REDACTED]"],
  [/([?&](?:api[_-]?key|access[_-]?token|auth[_-]?token|token|key)=)[^&\s]+/gi, "$1[REDACTED]"],
  [/\bBearer\s+[^\s,;]+/gi, "Bearer [REDACTED]"],
  [/\b(?:sk|rk|pk)-[A-Za-z0-9_-]{12,}\b/g, "[REDACTED]"]
];

function sanitizeErrorText(value: string): string {
  return SECRET_PATTERNS.reduce(
    (sanitized, [pattern, replacement]) => sanitized.replace(pattern, replacement),
    value
  );
}

function currentLocation(): string {
  return `${window.location.pathname}${window.location.hash}`;
}

function describeError(error: unknown): { message: string; details: string } {
  if (error instanceof Error) {
    return {
      message: sanitizeErrorText(error.message || error.name),
      details: sanitizeErrorText(error.stack || `${error.name}: ${error.message}`)
    };
  }

  const message = sanitizeErrorText(String(error || "Error desconocido"));
  return { message, details: message };
}

function createIncident(error: unknown, source: ErrorSource): ErrorIncident {
  const description = describeError(error);
  return {
    ...description,
    source,
    occurredAt: new Date().toISOString(),
    location: currentLocation()
  };
}

function incidentReport(incident: ErrorIncident): string {
  return [
    "Rau Studio - reporte de error",
    `Fecha: ${incident.occurredAt}`,
    `Origen: ${incident.source}`,
    `Pantalla: ${incident.location}`,
    `Mensaje: ${incident.message}`,
    "",
    incident.details
  ].join("\n");
}

async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Some desktop webviews expose the Clipboard API but deny it without focus.
  }

  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.setAttribute("readonly", "");
  textArea.style.position = "fixed";
  textArea.style.opacity = "0";
  document.body.appendChild(textArea);
  textArea.select();

  try {
    return document.execCommand("copy");
  } finally {
    textArea.remove();
  }
}

export class AppErrorBoundary extends Component<AppErrorBoundaryProps, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = {
    incident: null,
    copied: false
  };

  static getDerivedStateFromError(error: unknown): Partial<AppErrorBoundaryState> {
    return { incident: createIncident(error, "render"), copied: false };
  }

  componentDidMount() {
    window.addEventListener("error", this.handleWindowError);
    window.addEventListener("unhandledrejection", this.handleUnhandledRejection);
  }

  componentWillUnmount() {
    window.removeEventListener("error", this.handleWindowError);
    window.removeEventListener("unhandledrejection", this.handleUnhandledRejection);
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const incident = createIncident(error, "render");
    const componentStack = info.componentStack?.trim();
    this.setState({
      incident: {
        ...incident,
        details: componentStack
          ? `${incident.details}\n\nComponent stack:\n${sanitizeErrorText(componentStack)}`
          : incident.details
      },
      copied: false
    });
  }

  handleWindowError = (event: ErrorEvent) => {
    event.preventDefault();
    this.setState({
      incident: createIncident(event.error ?? event.message, "window"),
      copied: false
    });
  };

  handleUnhandledRejection = (event: PromiseRejectionEvent) => {
    event.preventDefault();
    this.setState({
      incident: createIncident(event.reason, "promise"),
      copied: false
    });
  };

  copyReport = async () => {
    if (!this.state.incident) return;

    const copied = await copyText(incidentReport(this.state.incident));
    this.setState({ copied });
  };

  retry = () => {
    this.setState({ incident: null, copied: false });
  };

  render() {
    const { incident } = this.state;
    if (!incident) return this.props.children;

    return (
      <main className="grid min-h-screen place-items-center bg-background p-4 text-foreground">
        <section className="w-full max-w-2xl rounded-lg border border-red-300 bg-card p-5 shadow-lg dark:border-red-900">
          <div className="flex items-start gap-3">
            <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full bg-red-100 text-red-700 dark:bg-red-950 dark:text-red-200">
              <AlertTriangle className="h-5 w-5" />
            </span>
            <div className="min-w-0">
              <h1 className="m-0 text-xl font-semibold">Rau Studio encontró un error</h1>
              <p className="mt-1 text-sm text-muted-foreground">
                La operación se detuvo para evitar dejar la aplicación en un estado inconsistente.
              </p>
            </div>
          </div>

          <div className="mt-4 rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-900 dark:border-red-900 dark:bg-red-950/40 dark:text-red-100">
            <strong className="block text-xs uppercase tracking-wide">Mensaje</strong>
            <span className="mt-1 block break-words font-mono text-xs">{incident.message}</span>
          </div>

          <dl className="mt-3 grid gap-1 text-xs text-muted-foreground sm:grid-cols-[90px_minmax(0,1fr)]">
            <dt className="font-semibold text-foreground">Pantalla</dt>
            <dd className="m-0 break-all font-mono">{incident.location}</dd>
            <dt className="font-semibold text-foreground">Fecha</dt>
            <dd className="m-0 font-mono">{incident.occurredAt}</dd>
          </dl>

          <details className="mt-4 rounded-md border border-border bg-muted/40 p-3">
            <summary className="cursor-pointer text-sm font-semibold">Ver detalle técnico</summary>
            <pre className="mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-words text-xs text-muted-foreground">
              {incident.details}
            </pre>
          </details>

          <p className="mt-3 text-xs text-muted-foreground">
            Las credenciales reconocibles se ocultan automáticamente del reporte.
          </p>

          <div className="mt-4 flex flex-wrap gap-2">
            <Button type="button" onClick={this.retry}>
              <RotateCcw className="h-4 w-4" />
              Intentar de nuevo
            </Button>
            <Button type="button" variant="secondary" onClick={() => void this.copyReport()}>
              <Copy className="h-4 w-4" />
              {this.state.copied ? "Detalles copiados" : "Copiar detalles"}
            </Button>
            <Button type="button" variant="ghost" onClick={() => window.location.reload()}>
              <RefreshCcw className="h-4 w-4" />
              Recargar aplicación
            </Button>
          </div>
        </section>
      </main>
    );
  }
}
