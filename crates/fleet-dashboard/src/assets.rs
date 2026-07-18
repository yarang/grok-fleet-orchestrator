//! 정적 자산 임베드.
//!
//! `assets/` 디렉토리의 파일들을 바이너리에 임베드합니다.
//! 빌드 시점에 파일이 존재하지 않으면 빈 디렉토리로 간주 (CI 허용).

use rust_embed::RustEmbed;

/// 임베드된 정적 자산. `assets/` 폴더가 빌드 컨텍스트에 존재해야 함.
#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Asset;
