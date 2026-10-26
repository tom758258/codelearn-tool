import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import codelearnIcon from '../src-tauri/icons/codelearn-icon.png'
import './styles.css'

function App() {
  return (
    <main>
      <img className="mark" src={codelearnIcon} alt="" />
      <h1>CodeLearn Tool</h1>
      <p>A place to learn programming, starting with C.</p>
      <p className="status">The desktop foundation is ready. Learning tools are coming later.</p>
    </main>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
