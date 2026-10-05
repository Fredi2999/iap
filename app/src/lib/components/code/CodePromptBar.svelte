<script lang="ts">
  // Dieselbe Eingabeleiste wie im Chat, nur ohne die Chat-Schalter (Modus, Werkzeuge, Denkstufe).
  // Die Modellwahl wirkt auf das aktive Modell der ganzen App.
  import { onMount } from "svelte";
  import PromptBar from "../PromptBar.svelte";
  import { codeModels, loadCodeModels, switchCodeModel } from "../../stores/codeModels.svelte";
  import type { SettingsSnapshot } from "../../types";

  interface Props {
    value: string;
    /** Eine Aufgabe läuft gerade; die Leiste zeigt dann den Stopp-Knopf. */
    busy: boolean;
    /** Sperrt die Modellwahl, solange anderswo im Code-Bereich gearbeitet wird. */
    locked?: boolean;
    /** Bereits übersetzter Platzhaltertext. */
    placeholder: string;
    settings: SettingsSnapshot;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
    onOpenSettings: () => void;
    onSend: () => void;
    onStop: () => void;
    onError?: (reason: unknown) => void;
    /** Eindeutige Kennung des Eingabefelds (der Code-Bereich zeigt mehrere Leisten). */
    inputId: string;
  }

  let { value = $bindable(""), busy, locked = false, placeholder, settings, onSettingsChanged, onOpenSettings, onSend, onStop, onError, inputId }: Props = $props();

  onMount(async () => {
    try {
      await loadCodeModels();
    } catch (reason) {
      onError?.(reason);
    }
  });

  async function choose(id: string) {
    try {
      const snapshot = await switchCodeModel(id, settings.model_id);
      if (snapshot) onSettingsChanged(snapshot);
    } catch (reason) {
      onError?.(reason);
    }
  }
</script>

<PromptBar
  variant="code"
  {inputId}
  bind:value
  {placeholder}
  {busy}
  models={codeModels.models}
  modelId={settings.model_id}
  modelBusy={codeModels.busy || locked}
  onSelectModel={choose}
  {onOpenSettings}
  onSend={(event) => { event.preventDefault(); onSend(); }}
  {onStop}
  onDictate={(text) => { value = value ? `${value} ${text}` : text; }}
/>
