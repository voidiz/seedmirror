export function ErrorMessage({ text }: { text: string }) {
  return (
    <div className="mb-4 p-3 bg-destructive/10 border border-destructive text-destructive rounded-md text-sm">
      {text}
    </div>
  );
}
