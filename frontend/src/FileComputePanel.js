import { FILE_INPUT_LIMIT } from "./liveBridge";

export default function FileComputePanel({ task, preference, otherRunning, language }) {
  const fr = language !== "en";
  const allowed = preference.fresh && !preference.pending && preference.enabled === true;
  const valid = task.file && task.file.size > 0 && task.file.size <= FILE_INPUT_LIMIT;
  return <section aria-labelledby="file-compute-title" className="my-5 rounded-xl border border-[#202c35] bg-[#0a1015] p-5" data-testid="file-compute-panel">
    <h2 id="file-compute-title" className="font-semibold text-white">{fr ? "Calculer l’empreinte d’un fichier" : "Compute a file fingerprint"}</h2>
    <p className="mt-2 text-sm text-zinc-400">{fr ? "BLAKE3 · 1 octet à 128 Mio. Le daemon choisit le calcul distant ou le fallback local. Aucun gain de vitesse n’est promis." : "BLAKE3 · 1 byte to 128 MiB. The daemon chooses remote execution or local fallback. No speedup is promised."}</p>
    <label className="mt-4 block text-sm text-zinc-300">
      {fr ? "Fichier à calculer" : "File to compute"}
      <input type="file" data-testid="file-compute-input" className="mt-2 block w-full text-sm" onChange={(event) => task.select(event.target.files?.[0] || null)} />
    </label>
    {task.file && <p className="mt-2 break-all text-sm text-zinc-400">{task.file.name} · {task.file.size} {fr ? "octets" : "bytes"}</p>}
    <button type="button" data-testid="run-file-compute" disabled={!valid || !allowed || task.running || otherRunning} onClick={task.run} className="mt-4 rounded-lg bg-primary px-4 py-2 text-sm font-bold text-black disabled:bg-zinc-800 disabled:text-zinc-500">
      {task.running ? (fr ? "Transfert ou calcul en cours…" : "Transfer or computation pending…") : (fr ? "Calculer BLAKE3" : "Compute BLAKE3")}
    </button>
    {!allowed && <p className="mt-2 text-sm text-amber-200">{fr ? "Une préférence ON fraîche et confirmée est nécessaire." : "A fresh, confirmed ON preference is required."}</p>}
    <div aria-live="polite">
      {task.error && <p className="mt-3 text-sm text-amber-200" data-testid="file-compute-error">{task.error === "INVALID_FILE_SIZE" ? (fr ? "Choisissez un fichier de 1 octet à 128 Mio." : "Choose a file from 1 byte to 128 MiB.") : task.error === "OUTCOME_UNKNOWN" ? (fr ? "Résultat inconnu. Le calcul peut avoir eu lieu. Aucun renvoi automatique." : "Outcome unknown. Computation may have occurred. No automatic replay.") : (fr ? "Demande non aboutie. Aucun renvoi automatique." : "Request did not complete. No automatic replay.")} <span className="font-mono">{task.error}</span></p>}
      {task.result && <div className="mt-3 break-all text-sm text-zinc-300" data-testid="file-compute-result">
        <p>{task.result.execution_source === "REMOTE_SUCCESS" ? (fr ? "Calcul distant confirmé" : "Confirmed remote computation") : (fr ? "Fallback local confirmé" : "Confirmed local fallback")}</p>
        <p className="font-mono">{task.result.execution_source} · {task.result.auto_use_reason}</p>
        <p>{task.result.input_bytes} {fr ? "octets" : "bytes"}</p>
        <p className="font-mono">{task.result.digest_blake3_hex}</p>
      </div>}
    </div>
  </section>;
}
