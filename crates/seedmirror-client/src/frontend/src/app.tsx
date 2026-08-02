import { ThemeProvider } from "./components/theme-provider";
import { Index } from "./pages";

function App() {
  return (
    <ThemeProvider defaultTheme="dark" storageKey="vite-ui-theme">
      <Index />
    </ThemeProvider>
  );
}

export default App;
