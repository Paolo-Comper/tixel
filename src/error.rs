use std::fmt;

#[derive(Debug)]
pub enum ImgToTerminalError {
    FileNotFound(String),
    UnknownFormat(String),
    VideoOpen(String),
    VideoDecode(String),
    VideoInit(String),
    ImageLoad(String),
    NoFrames(String),
    TerminalTooSmall(u16, u16),
    IoError(std::io::Error),
}

impl fmt::Display for ImgToTerminalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImgToTerminalError::FileNotFound(path) => {
                write!(
                    f,
                    "File non trovato: '{path}'.\n\
                     Assicurati che il percorso esista e sia leggibile."
                )
            }
            ImgToTerminalError::UnknownFormat(ext) => {
                write!(
                    f,
                    "Formato '{ext}' non supportato.\n\
                     Formati video supportati: mp4, mov, avi, webm, mkv\n\
                     Formati immagine supportati: png, jpg, jpeg, bmp, gif"
                )
            }
            ImgToTerminalError::VideoOpen(detail) => {
                write!(
                    f,
                    "Impossibile aprire il video: {detail}.\n\
                     Verifica che il file non sia corrotto e che FFmpeg sia installato."
                )
            }
            ImgToTerminalError::VideoDecode(detail) => {
                write!(
                    f,
                    "Errore durante la decodifica del video: {detail}."
                )
            }
            ImgToTerminalError::VideoInit(detail) => {
                write!(
                    f,
                    "Impossibile inizializzare il motore video: {detail}.\n\
                     Assicurati che FFmpeg sia installato sul sistema."
                )
            }
            ImgToTerminalError::ImageLoad(detail) => {
                write!(
                    f,
                    "Impossibile caricare l'immagine: {detail}.\n\
                     Verifica che il file non sia corrotto."
                )
            }
            ImgToTerminalError::NoFrames(path) => {
                write!(
                    f,
                    "Nessun frame valido trovato in '{path}'.\n\
                     Il file potrebbe essere vuoto o illeggibile."
                )
            }
            ImgToTerminalError::TerminalTooSmall(cols, rows) => {
                write!(
                    f,
                    "Terminale troppo piccolo ({cols}col x {rows}righe).\n\
                     Servono almeno 20 colonne per visualizzare qualcosa."
                )
            }
            ImgToTerminalError::IoError(err) => {
                write!(f, "Errore di I/O: {err}.")
            }
        }
    }
}

impl std::error::Error for ImgToTerminalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImgToTerminalError::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ImgToTerminalError {
    fn from(err: std::io::Error) -> Self {
        ImgToTerminalError::IoError(err)
    }
}

impl From<image::ImageError> for ImgToTerminalError {
    fn from(err: image::ImageError) -> Self {
        ImgToTerminalError::ImageLoad(err.to_string())
    }
}
