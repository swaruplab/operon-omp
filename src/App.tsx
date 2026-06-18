import { useEffect, useState } from 'react';
import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';
import { AppShell } from './components/layout/AppShell';
import { ProjectProvider } from './context/ProjectContext';
import { SetupWizard } from './components/setup/SetupWizard';
import { getSettings } from './lib/settings';

// Configure Monaco to use the local bundle instead of CDN.
// This is critical for Tauri because CSP blocks external scripts.
loader.config({ monaco });

function App() {
  // null = still loading settings; once loaded, true/false drives the gate.
  const [setupComplete, setSetupComplete] = useState<boolean | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => setSetupComplete(s.setup_completed === true))
      .catch(() => setSetupComplete(true)); // fail-open: don't block on settings read errors
  }, []);

  if (setupComplete === null) {
    return <div className="fixed inset-0 bg-zinc-950" />;
  }

  if (!setupComplete) {
    return <SetupWizard onComplete={() => setSetupComplete(true)} />;
  }

  return (
    <ProjectProvider>
      <AppShell />
    </ProjectProvider>
  );
}

export default App;
