use crate::model::PresentationModel;
use crate::Error;

pub fn open(path: &str) -> Result<PresentationModel, Error> {
    crate::open_pptx(path)
}

pub fn save(path: &str, model: &PresentationModel) -> Result<(), Error> {
    crate::save_pptx(path, model)
}
