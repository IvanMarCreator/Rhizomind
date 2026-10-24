/** Полный список IPC-каналов. Main обрабатывает только их, остальное игнорируется. */
export const IPC = {
  vaultOpen: 'vault:open',
  vaultCurrent: 'vault:current',
  vaultListNotes: 'vault:listNotes',
  vaultReadNote: 'vault:readNote',
  vaultWriteNote: 'vault:writeNote',
  vaultCreateNote: 'vault:createNote',
  linksResolve: 'links:resolve',
  linksBacklinks: 'links:backlinks',
} as const;
