/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, AsyncSmtpTransport, Transport, AsyncTransport, Tokio1Executor};
use lettre::message::{Mailbox, MultiPart, SinglePart, Attachment, Body, header::ContentType};
use std::{fs, path::{Path,PathBuf}, time::Duration};
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::time::timeout;

use odin_actor::{warn,error};
use odin_common::if_let;
use crate::{op_failed, Alarm, AlarmMessenger, EvidenceInfo, OdinSentinelError};
use crate::errors::Result;

 #[derive(Deserialize,Serialize,Debug)]
pub struct SmtpConfig {
    pub smtp_uri: String,
    pub username: String,
    pub pw: String,
    pub sender: String,
    pub recipients: Vec<String>,
    pub timeout: Duration,
}

 /// SMTP based AlarmMessenger
 /// this sends alarm notifications as email, which also supports email-to-text message gateways
 /// for most cellular providers, e.g. by sending email to:
 /// - ATT: <10-digit-phone>@txt.att.net (text) or <10-digit-phone>@mms.att.net (mms) 
 /// - T-Mobile: <10-digit-phone>@tmomail.net) (text + mms)
 /// - Verizon: <10-digit-phone>@vtext.com (text) or <10-digit-phone>@vzwpix.com (mms)
 /// - Sprint: <10-digit-phone>@messaging.sprintpcs.com (text) or <10-digit-phone>@pm.sprint.com (mms)
pub struct SmtpAlarmMessenger {
    config: SmtpConfig,

    from_addr: Mailbox,
    bcc_addrs: Vec<Mailbox>
}

impl SmtpAlarmMessenger {
    pub fn new (config: SmtpConfig)->Self {
        // compute those first since there is not point sending alarms if they are wrong
        let from_addr = config.sender.parse::<Mailbox>().unwrap(); // this is a top level object, panic is Ok
        let bcc_addrs: Vec<Mailbox> = config.recipients.iter().map(|r| r.parse::<Mailbox>().unwrap()).collect();
        if bcc_addrs.is_empty() { warn!("no alarm receiver configured" )}

        SmtpAlarmMessenger { config, from_addr, bcc_addrs }
    }
}

#[async_trait]
impl AlarmMessenger for SmtpAlarmMessenger {
    async fn send_alarm (&self, alarm: &Alarm)->Result<()> {
        let config = &self.config;

        let creds = Credentials::new( config.username.clone(), config.pw.clone());
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay( config.smtp_uri.as_str())
        //let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay( config.smtp_uri.as_str())
        //let mailer = SmtpTransport::relay( config.smtp_uri.as_str())
            .map_err(|e| op_failed!("could not connect to SMTP: {e}"))?
            .credentials(creds)
            .build();

        let evidences = get_attachments(&alarm);
        let message = create_message(&self.from_addr, &self.bcc_addrs, "alarm", &alarm.description, &evidences)?;

        let response = timeout( self.config.timeout, mailer.send(message)).await??;
        if response.is_positive() { Ok(()) } else { Err( OdinSentinelError::SmtpError( format!("{response:?}"))) }
    }
}

 pub fn get_attachments (alarm: &Alarm)->Vec<(String,PathBuf)> {
    alarm.evidence_info.iter().fold( Vec::<(String,PathBuf)>::new(), |mut acc, e|{
        if let Some(sentinel_file) = &e.img {
            if sentinel_file.pathname.is_file() {
                acc.push( (e.description.clone(), sentinel_file.pathname.clone()))
            }
        }
        acc
    })
}

fn create_message (sender: &Mailbox, recipients: &Vec<Mailbox>, subject: &str, text: &str, evidences: &Vec<(String,PathBuf)>)->Result<Message> {
    let mut parts: MultiPart = MultiPart::related().singlepart( SinglePart::plain(text.to_string()));
    for (img_title,pathbuf) in evidences {
        if_let! {
            Ok(mime_type) = get_mime_type(&pathbuf),
            Ok(img) = fs::read(pathbuf) => {
                //let attachment = Attachment::new_inline( img_title.clone()).body( Body::new(img), mime_type); // this apparently kills delivery for gmail
                let attachment = Attachment::new( pathbuf.to_string_lossy().to_string()).body( Body::new(img), mime_type);
                parts = parts.singlepart( attachment);
            }
        }
    }

    let mut mb = Message::builder().from(sender.clone()).subject( subject);
    for receiver in recipients {
        mb = mb.bcc(receiver.clone());
    }

    mb.multipart( parts).map_err( |e| op_failed!("failed to construct email: {e:?}"))
}

fn get_mime_type (path: &Path)->Result<ContentType> {
    let ext = path.extension().and_then( |e| e.to_str()).ok_or(op_failed!("no image file extension"))?;
    
    match ext {
        "webp" => Ok("image/webp".parse().unwrap()), // we know it's valid but we can't create a ContentType explicitly
        "png"  => Ok("image/png".parse().unwrap()), // ditto
        "jpeg" => Ok("image/jpeg".parse().unwrap()), // ditto
        "gif" => Ok("image/gif".parse().unwrap()), // ditto
        
        other => Err(op_failed!("unsupported image type {other}"))
    }
}