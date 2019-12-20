use id3::Frame;
use id3::frame::Content;

/// Translates a vorbis comment to the corresponding ID3v2.3 frame.
/// Source for the mappings: https://wiki.hydrogenaud.io/index.php?title=Tag_Mapping
pub fn translate_vorbis_comment_to_id3(
    vorbis_name: &String, vorbis_value: &String
) -> Option<Frame> {
    match vorbis_name.to_uppercase().as_ref() {
        "ALBUM" => Some(Frame::with_content("TALB", Content::Text(vorbis_value.clone()))),
        "TITLE" => Some(Frame::with_content("TIT2", Content::Text(vorbis_value.clone()))),
        "ALBUMARTIST" => Some(Frame::with_content("TPE2", Content::Text(vorbis_value.clone()))),
        "ARTIST" => Some(Frame::with_content("TPE1", Content::Text(vorbis_value.clone()))),
        "TRACKNUMBER" => Some(Frame::with_content("TRCK", Content::Text(vorbis_value.clone()))),
        "YEAR" => Some(Frame::with_content("TYER", Content::Text(vorbis_value.clone()))),
        "ISRC" => Some(Frame::with_content("TSRC", Content::Text(vorbis_value.clone()))),
        "GENRE" => Some(Frame::with_content("TCON", Content::Text(vorbis_value.clone()))),
        "COMMENT" => Some(Frame::with_content("COMM", Content::Text(vorbis_value.clone()))),
        "COPYRIGHT" => Some(Frame::with_content("TCOP", Content::Text(vorbis_value.clone()))),
        _ => {
            info!("No corresponding ID3v2.3 tag found for vorbis comment {}, ignoring", vorbis_name);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tags::translate_vorbis_comment_to_id3;

    use id3::Frame;
    use id3::frame::Content;

   #[test]
   fn test_translate_vorbis_comment_to_id3() {
       // Tag with only ASCII characters in the value
       let expected = Some(Frame::with_content("TALB", Content::Text(String::from("Polychrome"))));
       let actual = translate_vorbis_comment_to_id3(&String::from("Album"), &String::from("Polychrome"));
       assert_eq!(expected, actual);

       // Tag with non-ASCII characters in the value
       let expected = Some(Frame::with_content("TALB", Content::Text(String::from("नमस्ते"))));
       let actual = translate_vorbis_comment_to_id3(&String::from("Album"), &String::from("नमस्ते"));
       assert_eq!(expected, actual);

       // Tag with no mapping
       let expected = None;
       let actual = translate_vorbis_comment_to_id3(&String::from("Not a vorbis comment"), &String::from(""));
       assert_eq!(expected, actual);
   }
}
