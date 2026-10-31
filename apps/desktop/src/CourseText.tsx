import { useEffect, useState } from 'react'
import Markdown from 'react-markdown'
import { invoke } from '@tauri-apps/api/core'

type Location = { course: string; chapter: string; block: string }

function CourseImage({ location, reference, alt }: { location: Location; reference: string; alt: string }) {
  const [source, setSource] = useState('')
  const [error, setError] = useState('')
  useEffect(() => {
    let active = true
    setSource(''); setError('')
    void invoke<number[]>('get_course_image', { key: location.course, chapter: location.chapter, block: location.block, reference })
      .then(bytes => {
        // Only the host-validated local raster bytes become an image URL.
        const data = Uint8Array.from(bytes)
        const mime = data[0] === 137 ? 'image/png' : data[0] === 255 ? 'image/jpeg' : data[0] === 71 ? 'image/gif' : 'image/webp'
        const reader = new FileReader()
        reader.onload = () => { if (active) setSource(String(reader.result)) }
        reader.readAsDataURL(new Blob([data], { type: mime }))
      }).catch(cause => { if (active) setError(String(cause)) })
    return () => { active = false }
  }, [location.course, location.chapter, location.block, reference])
  return source ? <img src={source} alt={alt} /> : <span className={error ? 'image-error' : 'hint'}>{error ? `${alt || 'Image'}: ${error}` : `Loading ${alt || 'image'}…`}</span>
}

export default function CourseText({ location, text }: { location: Location; text: string }) {
  return <div className="course-text"><Markdown skipHtml components={{
    img: ({ src, alt }) => <CourseImage location={location} reference={typeof src === 'string' ? src : ''} alt={alt ?? ''} />,
    a: ({ children, href }) => <span className="reading-link" title={href}>{children}{href ? ` (${href})` : ''}</span>,
  }}>{text}</Markdown></div>
}
